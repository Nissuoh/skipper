// SPDX-License-Identifier: MIT
//! `cargo xtask stage1-check` (Stage 1 Task 45; MC-131, MC-134, MC-139, S1G-16, S1G-17): the runner of the signed Stage 1
//! checklist. It reads the checklist (`docs/probes/stage-1/checklist.md`) and the answers (`results.md`), says whether Stage 1
//! is done, records an answer and copies the signed probe values into `probe-facts.toml`.
//!
//! ```text
//! cargo xtask stage1-check          -> stage1-check: ok - yes | ok - fallback used - <items> | open - <items> | no - <items>
//! cargo xtask stage1-check --record <ID> --result yes|no [--evidence <names>] [--value k=v]... [--fallback <text>]
//! cargo xtask stage1-check --record <ID> --result skip --reason <text> [--evidence <names>]
//! cargo xtask stage1-check --sign <name> --date <YYYY-MM-DD>
//! cargo xtask stage1-check --facts [<facts file>]           -> writes the signed values into probe-facts.toml
//!       every form takes [--checklist <file>] [--results <file>] [--evidence-dir <folder>]
//! ```
//!
//! **The checklist** has one item per check, a `### <ID> <title>` heading and one `- <Field>: <text>` line per field (`Run`,
//! `Look at`, `Expected`, `Evidence`, `Fallback`, `Where`, and where they apply `Needs`, `Only with`, `Fact`, `Keys`). It is
//! the one list of what Stage 1 needs: [`required_ids`] names the items that may never be dropped from it, and [`gate_ids`]
//! the items that must stay gates (`Fallback: none`).
//!
//! **The results** are one table row per item: `yes`, `no` or `skip`, the evidence file names, the values of the fact
//! (`key=value; key=value`) and, in the last cell, for a no the fallback taken and for a skip the reason; plus the two
//! switches `S1-PROBES` and `OQ1` and the `Signed:` line. `not run` is no answer: a probe that was not run stays open.
//!
//! **The verdict.** An item is open until it is answered; a yes is open without every evidence file the checklist names
//! (each an existing, non-empty file in the evidence folder whose name starts with the item id) and without a good value
//! for every key the item carries; a no on an item with `Fallback: none` is a no; a no on an item with a fallback text is a
//! fallback used and needs that text (it fails nothing by itself, MC-134); a no needs a value for every key with a list for
//! a no (`key=a|b/c|d`); a `skip` of an item with a fallback text needs its reason, carries no values and no fact, and holds
//! the fallback; an item with `Fallback: none` (a gate) can be neither skipped nor left `not run`; a `not run` is open until
//! the probe is run or skipped (Lead ruling R4); an item whose `Needs` is not yes stays open (the
//! cap test PRB-25 and the second-session test PRB-46 gate S1-6 and S1-8 this way, plan Task 45 ruling); an item with
//! `Only with: <switch>` is not required while the switch says no (and is open while it says nothing). Any no gives
//! `no`, else any open item, an unsigned file or a bad row gives `open`, else `ok`. `ok` is never the plain yes while a
//! fallback holds: with a fallback used or a skip the first line is `ok - fallback used - <items>` and every such item is
//! listed with its fallback text or its reason (R4). Exit codes: 0 ok, 1 anything else, 2 usage.
//!
//! **The facts.** `--facts` writes, for every answered item that names a `Fact`, the status (`passed` for a yes, `failed` for
//! a no) and `source` (the result row and the signing date) and the values of its keys into the table of that name, keeping
//! every comment of the file and adding a table that is missing. It writes nothing for an open item and nothing at all
//! while the results are unsigned. The file is read again after the write; a table that does not read back as written is an
//! error and the file stays as it was.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Where the files are when no option names them (the workspace root is the current folder of `cargo xtask`).
pub const DEFAULT_CHECKLIST: &str = "docs/probes/stage-1/checklist.md";
pub const DEFAULT_RESULTS: &str = "docs/probes/stage-1/results.md";
pub const DEFAULT_EVIDENCE: &str = "docs/probes/stage-1/evidence";
pub const DEFAULT_FACTS: &str = "crates/skipper-core/data/probe-facts.toml";

/// The switches of the results file that an item may name in `Only with`.
pub const SWITCHES: [&str; 2] = ["S1-PROBES", "OQ1"];

/// The fields an item may have, and the ones it must have.
const FIELDS: [&str; 11] =
    ["Spec", "Where", "Run", "Look at", "Expected", "Evidence", "Fallback", "Needs", "Only with", "Fact", "Keys"];
const REQUIRED_FIELDS: [&str; 6] = ["Where", "Run", "Look at", "Expected", "Evidence", "Fallback"];

/// The extensions an evidence file may have.
const EVIDENCE_EXTENSIONS: [&str; 6] = ["txt", "md", "log", "csv", "json", "png"];

const USAGE: &str = "usage: cargo xtask stage1-check [--checklist <file>] [--results <file>] [--evidence-dir <folder>] \
(--record <ID> --result yes|no [--evidence <names>] [--value key=value]... [--fallback <text>] \
| --record <ID> --result skip --reason <text> [--evidence <names>] \
| --sign <name> --date <YYYY-MM-DD> | --facts [<facts file>])";

/// The two probes of the stress tests and the two criteria that need both of them (plan Task 45 ruling): the checker refuses a
/// checklist in which a `Needs` line was dropped, so S1-6 and S1-8 cannot pass without the stress probes.
const STRESS_PROBES: [&str; 2] = ["PRB-25", "PRB-46"];
const STRESS_CRITERIA: [&str; 2] = ["S1-6", "S1-8"];
/// The only gate that may stand behind a switch (`Only with: OQ1`): the owner's answer to the open question 1.
const SWITCHED_GATE: (&str, &str) = ("S1-35", "OQ1");

/// The items that every checklist must hold: the criteria of MC-131, the hand walks, the login-status check and the probes of
/// Stage 1 (plan Task 45 and its addendum amendment). A checklist that lacks one of them is refused, so that no item can be
/// dropped to reach `ok`.
pub fn required_ids() -> Vec<String> {
    let mut ids: Vec<String> = vec!["M-1".to_string(), "M-2".to_string(), "M-3".to_string()];
    ids.extend((1..=35).map(|n| format!("S1-{n}")));
    ids.extend(["S1-5a", "S1-5b", "S1-5c", "S1-5d"].map(String::from));
    ids.extend(["UI-50-9", "UI-50-10", "UI-50-11", "UI-44", "UI-46", "LOGIN-1"].map(String::from));
    ids.extend(
        [
            "PRB-04",
            "PRB-07",
            "PRB-08",
            "PRB-09",
            "PRB-12",
            "PRB-20",
            "PRB-21",
            "PRB-22",
            "PRB-23",
            "PRB-25",
            "PRB-26",
            "PRB-27",
            "PRB-29",
            "PRB-30",
            "PRB-40",
            "PRB-40b",
            "PRB-40c",
            "PRB-40d",
            "PRB-40e",
            "PRB-41",
            "PRB-42",
            "PRB-43",
            "PRB-44",
            "PRB-45",
            "PRB-46",
            "PRB-49",
            "PRB-50",
            "PRB-51",
            "PRB-52",
            "PRB-53-helpers",
            "PRB-56",
            "PRB-57",
            "PRB-59",
            "PRB-63",
            "PRB-64",
            "PRB-66",
            "PRB-67",
            "PRB-79",
            "MC-10-effort",
        ]
        .map(String::from),
    );
    ids.extend((80..=98).map(|n| format!("PRB-{n}")));
    ids.extend((100..=102).chain(104..=114).map(|n| format!("PRB-{n}")));
    ids.push("PRB-DUTY-4".to_string());
    ids
}

/// The probes among [`required_ids`] that are gates. A gate answered no is a no of Stage 1.
const GATE_PROBES: &str = "PRB-07 PRB-08 PRB-12 PRB-22 PRB-26 PRB-27 PRB-30 PRB-42 PRB-44 PRB-45 PRB-49 PRB-50 PRB-51 \
PRB-52 PRB-56 PRB-57 PRB-59 PRB-64 PRB-66 PRB-67 PRB-86 PRB-92 PRB-94 PRB-97 PRB-100 PRB-102";

/// The items that must say `Fallback: none`: the machine record, the build under test, the published history (M-3, Lead ruling
/// R5), the criteria of MC-131 (except the three card criteria S1-5a to S1-5c, which fall back with PRB-90 and
/// PRB-53-helpers), the hand walks and the gate probes. Whether an item is a gate is read from its `Fallback:` line, so a
/// checklist in which one of these carries a fallback text is refused: one edited line cannot turn a gate's no into a
/// fallback used. A gate may not stand behind a switch either (`Only with`), but S1-35 behind OQ1 (see [`check_checklist`]).
pub fn gate_ids() -> Vec<String> {
    let mut ids: Vec<String> = vec!["M-1".to_string(), "M-2".to_string(), "M-3".to_string()];
    ids.extend((1..=35).map(|n| format!("S1-{n}")));
    ids.push("S1-5d".to_string());
    ids.extend(["UI-50-9", "UI-50-10", "UI-50-11", "UI-44", "UI-46"].map(String::from));
    ids.extend(GATE_PROBES.split_whitespace().map(String::from));
    ids
}

// ---------------------------------------------------------------------------------------------------------
// The checklist
// ---------------------------------------------------------------------------------------------------------

/// One key a fact carries: `name`, `name?` (may stay empty), `name=a|b` (a yes needs one of these values), `name=a|b/c|d`
/// (and a no needs one of the second list). An empty list allows every value.
#[derive(Clone, Debug, PartialEq)]
pub struct KeySpec {
    pub name: String,
    pub optional: bool,
    pub yes: Vec<String>,
    pub no: Vec<String>,
}

/// One item of the checklist, as far as the checker reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    /// The line of its heading.
    pub line: usize,
    pub needs: Vec<String>,
    pub only_with: Option<String>,
    /// `None`: `Fallback: none`, a no is a no. `Some(text)`: what holds when the probe fails.
    pub fallback: Option<String>,
    /// The table of `probe-facts.toml` the signed answer fills.
    pub fact: Option<String>,
    pub keys: Vec<KeySpec>,
    /// The evidence files the item names; a yes cites every one.
    pub evidence: Vec<String>,
}

struct Draft {
    id: String,
    line: usize,
    fields: BTreeMap<String, String>,
}

/// An item id: an upper-case letter first, then letters, digits and single dashes (`S1-5a`, `PRB-40b`, `MC-10-effort`).
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > 40 || !bytes[0].is_ascii_uppercase() {
        return false;
    }
    if id.ends_with('-') || id.contains("--") {
        return false;
    }
    bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// Whether `name` may be an evidence file of the item: lower case, no folder, a known extension and the item id (lower
/// case) at the start, followed by a dash or the dot of the extension.
pub fn evidence_name_ok(id: &str, name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' || c == '_';
    if name.is_empty() || name.len() > 100 || !name.chars().all(allowed) || name.contains("..") {
        return false;
    }
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    if !EVIDENCE_EXTENSIONS.contains(&extension) {
        return false;
    }
    match name.strip_prefix(id.to_ascii_lowercase().as_str()) {
        Some(rest) => rest.starts_with('-') || rest.starts_with('.'),
        None => false,
    }
}

fn split_field(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("- ")?;
    let (name, value) = rest.split_once(':')?;
    Some((name.trim().to_string(), value.trim().to_string()))
}

/// The comma-separated words of a field.
fn comma_list(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn bar_list(list: &str) -> Vec<String> {
    list.split('|').map(str::trim).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn key_name_ok(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn parse_keys(text: &str, at: &str, problems: &mut Vec<String>) -> Vec<KeySpec> {
    let mut keys = Vec::new();
    for token in comma_list(text) {
        let (head, list) = match token.split_once('=') {
            Some((head, list)) => (head.trim().to_string(), Some(list.trim().to_string())),
            None => (token.clone(), None),
        };
        let optional = head.ends_with('?');
        let name = head.trim_end_matches('?').trim().to_string();
        if !key_name_ok(&name) {
            problems.push(format!("{at}: '{token}' is not a key (lower-case letters, digits and underscores)"));
            continue;
        }
        let (yes, no) = match list {
            None => (Vec::new(), Vec::new()),
            Some(list) => match list.split_once('/') {
                Some((yes, no)) => (bar_list(yes), bar_list(no)),
                None => (bar_list(&list), Vec::new()),
            },
        };
        keys.push(KeySpec { name, optional, yes, no });
    }
    keys
}

fn finish(draft: Draft, items: &mut Vec<Item>, problems: &mut Vec<String>) {
    let at = format!("line {}: {}", draft.line, draft.id);
    for field in REQUIRED_FIELDS {
        if draft.fields.get(field).is_none_or(String::is_empty) {
            problems.push(format!("{at}: the field '{field}' is missing or empty"));
        }
    }
    let value = |name: &str| draft.fields.get(name).cloned().unwrap_or_default();

    let evidence = comma_list(&value("Evidence"));
    for name in &evidence {
        if !evidence_name_ok(&draft.id, name) {
            problems.push(format!(
                "{at}: the evidence file '{name}' must be lower case, start with '{}-' (or be '{}.<ext>') and end in one of {}",
                draft.id.to_ascii_lowercase(),
                draft.id.to_ascii_lowercase(),
                EVIDENCE_EXTENSIONS.join(", ")
            ));
        }
    }

    let fallback_text = value("Fallback");
    let fallback = if fallback_text.is_empty() || fallback_text == "none" { None } else { Some(fallback_text) };

    let only_with = match value("Only with").as_str() {
        "" => None,
        switch if SWITCHES.contains(&switch) => Some(switch.to_string()),
        other => {
            problems.push(format!("{at}: 'Only with' names {other}, which is no switch (one of {})", SWITCHES.join(", ")));
            None
        }
    };

    let fact = match value("Fact").as_str() {
        "" => None,
        name if valid_id(name) => Some(name.to_string()),
        other => {
            problems.push(format!("{at}: '{other}' is not a name of a fact table"));
            None
        }
    };

    let keys = parse_keys(&value("Keys"), &at, problems);
    if !keys.is_empty() && fact.is_none() {
        problems.push(format!("{at}: 'Keys' needs a 'Fact'"));
    }

    items.push(Item {
        id: draft.id.clone(),
        line: draft.line,
        needs: comma_list(&value("Needs")),
        only_with,
        fallback,
        fact,
        keys,
        evidence,
    });
}

/// Reads the items of a checklist and the problems of its form. A heading `### <ID> <title>` opens an item and every line
/// up to the next heading must be one of its fields; prose may stand under the other headings.
pub fn parse_checklist(md: &str) -> (Vec<Item>, Vec<String>) {
    let mut items: Vec<Item> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut current: Option<Draft> = None;
    for (index, raw) in md.lines().enumerate() {
        let n = index + 1;
        let line = raw.trim_end();
        if line.starts_with('#') {
            if let Some(draft) = current.take() {
                finish(draft, &mut items, &mut problems);
            }
            if let Some(rest) = line.strip_prefix("### ") {
                let id = rest.split_whitespace().next().unwrap_or("");
                if valid_id(id) {
                    current = Some(Draft { id: id.to_string(), line: n, fields: BTreeMap::new() });
                } else {
                    problems.push(format!("line {n}: '{id}' is not an item id"));
                }
            }
            continue;
        }
        let Some(draft) = current.as_mut() else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        match split_field(line) {
            Some((name, value)) if FIELDS.contains(&name.as_str()) => {
                if draft.fields.insert(name.clone(), value).is_some() {
                    problems.push(format!("line {n}: {}: the field '{name}' stands twice", draft.id));
                }
            }
            Some((name, _)) => problems.push(format!("line {n}: {}: '{name}' is not a field of an item", draft.id)),
            None => problems.push(format!("line {n}: {}: this line is not a field (- <Field>: <text>)", draft.id)),
        }
    }
    if let Some(draft) = current.take() {
        finish(draft, &mut items, &mut problems);
    }
    (items, problems)
}

/// [`parse_checklist`] plus the rules over the whole list: an id stands once, `Needs` names items, a fact is filled by one
/// item, every id of [`required_ids`] is there, a gate ([`gate_ids`]) says `Fallback: none` and stands behind no switch
/// (S1-35 behind OQ1 excepted), S1-6 and S1-8 need both stress probes and those sit behind `S1-PROBES`.
pub fn check_checklist(md: &str) -> (Vec<Item>, Vec<String>) {
    let (items, mut problems) = parse_checklist(md);
    let mut first_line: BTreeMap<&str, usize> = BTreeMap::new();
    for item in &items {
        if let Some(first) = first_line.insert(item.id.as_str(), item.line) {
            problems.push(format!("line {}: the item {} stands twice (first at line {first})", item.line, item.id));
        }
    }
    let ids: BTreeSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
    let mut fact_of: BTreeMap<&str, &str> = BTreeMap::new();
    for item in &items {
        for need in &item.needs {
            if need == &item.id {
                problems.push(format!("{}: an item cannot need itself", item.id));
            } else if !ids.contains(need.as_str()) {
                problems.push(format!("{}: 'Needs' names {need}, which is not an item", item.id));
            }
        }
        if let Some(fact) = &item.fact {
            if let Some(other) = fact_of.insert(fact.as_str(), item.id.as_str()) {
                problems.push(format!("{}: the fact {fact} is also filled by {other}", item.id));
            }
        }
    }
    for required in required_ids() {
        if !ids.contains(required.as_str()) {
            problems.push(format!("the item {required} is missing from the checklist"));
        }
    }
    for gate in gate_ids() {
        for item in items.iter().filter(|item| item.id == gate) {
            if item.fallback.is_some() {
                problems.push(format!("line {}: {gate} is a gate and must say 'Fallback: none'", item.line));
            }
            // A gate behind a switch at no is not required, so one edited line would drop it: only S1-35 may name a switch, and
            // only OQ1 (MINOR F1 of the recheck).
            match item.only_with.as_deref() {
                Some(switch) if (gate.as_str(), switch) != SWITCHED_GATE => {
                    problems.push(format!("line {}: {gate} is a gate and may not say 'Only with: {switch}'", item.line));
                }
                _ => {}
            }
        }
    }
    // S1-6 and S1-8 pass only with both stress probes (plan Task 45 ruling): a dropped `Needs` entry would let them pass alone, and
    // the probes belong behind the switch of the person's yes to them.
    for item in &items {
        if STRESS_CRITERIA.contains(&item.id.as_str()) {
            for probe in STRESS_PROBES {
                if !item.needs.iter().any(|need| need == probe) {
                    problems.push(format!(
                        "line {}: {} must say 'Needs: {}' ({probe} is not listed)",
                        item.line,
                        item.id,
                        STRESS_PROBES.join(", ")
                    ));
                }
            }
        }
        if STRESS_PROBES.contains(&item.id.as_str()) && item.only_with.as_deref() != Some("S1-PROBES") {
            problems.push(format!("line {}: {} must say 'Only with: S1-PROBES'", item.line, item.id));
        }
    }
    (items, problems)
}

// ---------------------------------------------------------------------------------------------------------
// The results
// ---------------------------------------------------------------------------------------------------------

/// One answer: a row of the results table.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    /// `yes`, `no`, or anything else (the placeholder, a blank): not answered.
    pub result: String,
    pub evidence: Vec<String>,
    pub values: Vec<(String, String)>,
    /// Why the values cell could not be read, if it could not.
    pub values_error: Option<String>,
    pub fallback: String,
}

/// What `results.md` says.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Results {
    pub rows: BTreeMap<String, Row>,
    /// `S1-PROBES` and `OQ1`: `Some(true)` for yes, `Some(false)` for no, `None` while the line is not filled.
    pub switches: BTreeMap<String, Option<bool>>,
    /// The name and the date of the signature, when the line is filled.
    pub signed: Option<(String, String)>,
    pub problems: Vec<String>,
}

fn looks_like_date(s: &str) -> bool {
    s.len() == 10 && s.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
}

/// `<name> – <date>` with both filled; the dash may be an en dash, an em dash or a hyphen.
fn parse_signature(rest: &str) -> Option<(String, String)> {
    let rest = rest.trim();
    let date = rest.split_whitespace().next_back()?;
    if !looks_like_date(date) {
        return None;
    }
    let head = rest[..rest.len() - date.len()].trim();
    let name = head.trim_end_matches(|c: char| c == '-' || c == '\u{2013}' || c == '\u{2014}').trim();
    if name.is_empty() || name.contains(|c: char| matches!(c, '<' | '>' | '|')) {
        return None;
    }
    Some((name.to_string(), date.to_string()))
}

/// The cells of a table row.
fn cells_of(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|cell| cell.trim().to_string()).collect()
}

fn is_separator(cell: &str) -> bool {
    !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':')
}

fn file_names(cell: &str) -> Vec<String> {
    cell.split(|c: char| c == ',' || c == ';').map(str::trim).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn parse_values(cell: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for piece in cell.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((key, value)) = piece.split_once('=') else {
            return Err(format!("'{piece}' is not key=value"));
        };
        let key = key.trim();
        if !key_name_ok(key) {
            return Err(format!("'{key}' is not a key name"));
        }
        out.push((key.to_string(), value.trim().to_string()));
    }
    Ok(out)
}

/// Reads `results.md`: the signature, the switches and one row per table line that is not the header.
pub fn parse_results(md: &str) -> Results {
    let mut out = Results::default();
    for (index, raw) in md.lines().enumerate() {
        let n = index + 1;
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("Signed:") {
            out.signed = parse_signature(rest);
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            if SWITCHES.contains(&name) {
                let answer = match value.trim().to_ascii_lowercase().as_str() {
                    "yes" => Some(true),
                    "no" => Some(false),
                    _ => None,
                };
                out.switches.insert(name.to_string(), answer);
                continue;
            }
        }
        if !line.starts_with('|') {
            continue;
        }
        let cells = cells_of(line);
        if cells.first().is_some_and(|c| c == "Item") || cells.iter().all(|c| is_separator(c)) {
            continue;
        }
        if cells.len() != 5 {
            out.problems.push(format!("line {n}: a table row has {} cells, 5 are expected", cells.len()));
            continue;
        }
        let (values, values_error) = match parse_values(&cells[3]) {
            Ok(values) => (values, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let id = cells[0].clone();
        let row = Row {
            id: id.clone(),
            result: cells[1].to_ascii_lowercase(),
            evidence: file_names(&cells[2]),
            values,
            values_error,
            fallback: cells[4].clone(),
        };
        if out.rows.insert(id.clone(), row).is_some() {
            out.problems.push(format!("line {n}: the item {id} has two rows"));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------
// The verdict
// ---------------------------------------------------------------------------------------------------------

/// Where one item stands.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Signed yes with its evidence and values.
    Yes,
    /// A signed no on a probe that has a fallback (the fallback taken, from the row): the fallback holds, the fact is failed.
    /// It counts as ok only as the visible state "fallback used" (Lead ruling R4).
    Fallback(String),
    /// An explicit skip of a probe that has a fallback: the reason of the row, and the fallback of the checklist that holds.
    /// The fact stays as it is. It counts as ok only as "fallback used" too (R4); a `not run` is [`State::Open`] instead.
    Skipped { reason: String, fallback: String },
    /// A signed no on an item without a fallback.
    No(String),
    /// Not answered, or answered in a way that does not count (the reason).
    Open(String),
    /// Behind a switch that says no, and not answered.
    NotRequired,
}

/// The checked state of the whole checklist.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// One state per item, in the order of the checklist.
    pub states: Vec<(String, State)>,
    /// What stands open besides the items: a switch that is not filled, the signature.
    pub extras: Vec<(String, String)>,
    /// A row that names no item, a row that cannot be read.
    pub problems: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Every item is a yes.
    Ok,
    /// Nothing is open or no, but at least one item holds only through its fallback (a no taken or a skip): ok, shown as
    /// "fallback used" (Lead ruling R4). The exit code is 0 like `Ok`.
    FallbackUsed,
    Open,
    No,
}

/// A value the facts file can hold between quotes.
fn plain_value(value: &str) -> bool {
    !value.chars().any(|c| c == '"' || c == '\\' || c.is_control())
}

fn base_state(item: &Item, row: Option<&Row>, required: bool, evidence_ok: &dyn Fn(&str) -> bool) -> State {
    let unanswered =
        if required { State::Open("not answered".to_string()) } else { State::NotRequired };
    let Some(row) = row else {
        return unanswered;
    };
    // A `not run` is no answer (Lead ruling R4): the probe blocks until it is run, or skipped with a reason. A gate has no
    // fallback, so for a gate there is no skip either. Behind a switch at no the item is not required, so the row is not read.
    let (answer, skipped) = match row.result.as_str() {
        "yes" => (true, false),
        "no" => (false, false),
        "skip" if item.fallback.is_some() => (false, true),
        "skip" => {
            return if required {
                State::Open("a gate cannot be skipped (it has no fallback): answer yes or no".to_string())
            } else {
                State::NotRequired
            };
        }
        "not run" => {
            return if !required {
                State::NotRequired
            } else if item.fallback.is_some() {
                State::Open(format!(
                    "not run blocks until the probe is run (--record {id} --result yes|no ...) or skipped with a reason \
(--record {id} --result skip --reason <text>)",
                    id = item.id
                ))
            } else {
                State::Open("a gate cannot be left not run (it has no fallback and cannot be skipped): answer yes or no".to_string())
            };
        }
        _ => return unanswered,
    };
    let mut problems: Vec<String> = Vec::new();
    if let Some(error) = &row.values_error {
        problems.push(format!("the values cannot be read: {error}"));
    }
    for name in &row.evidence {
        if !evidence_name_ok(&item.id, name) {
            problems.push(format!("the evidence '{name}' does not start with {} or has a bad form", item.id.to_ascii_lowercase()));
        } else if !evidence_ok(name) {
            problems.push(format!("the evidence file {name} is missing or empty"));
        }
    }
    if answer {
        if row.evidence.is_empty() {
            problems.push("a yes needs evidence".to_string());
        } else {
            for needed in &item.evidence {
                if !row.evidence.contains(needed) {
                    problems.push(format!("the evidence {needed} is not cited"));
                }
            }
        }
    }

    if skipped {
        // A skip fills no fact: it needs its reason (the last cell) and carries no values. Evidence is optional (the sheet
        // that says why the probe was skipped), but a file that is named must exist.
        if row.fallback.trim().is_empty() {
            problems.push("a skip needs the reason (the last cell, --reason)".to_string());
        }
        if !row.values.is_empty() {
            problems.push("a skip carries no values".to_string());
        }
    } else {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (key, value) in &row.values {
            if !seen.insert(key.as_str()) {
                problems.push(format!("the key {key} stands twice"));
                continue;
            }
            let Some(spec) = item.keys.iter().find(|spec| spec.name == *key) else {
                problems.push(format!("{key} is not a key of this item"));
                continue;
            };
            if value.is_empty() {
                continue;
            }
            if !plain_value(value) {
                problems.push(format!("the value of {key} holds a quote, a backslash or a control character"));
                continue;
            }
            let allowed = if answer { &spec.yes } else { &spec.no };
            if !allowed.is_empty() && !allowed.contains(value) {
                problems.push(format!(
                    "{key} = {value} is not allowed for a {} (one of: {})",
                    if answer { "yes" } else { "no" },
                    allowed.join(", ")
                ));
            }
        }
        if answer {
            for spec in &item.keys {
                let given = row.values.iter().any(|(key, value)| *key == spec.name && !value.is_empty());
                if !spec.optional && !given {
                    problems.push(format!("a yes needs a value for {}", spec.name));
                }
            }
        } else {
            if item.fallback.is_some() && row.fallback.trim().is_empty() {
                problems.push("a no needs the fallback taken".to_string());
            }
            // A key with a list for a no (`key=a|b/c|d`) must be given on a no as well, so the fact says which part failed: an
            // empty value would read as a part that was not run (PRB-89 `t_k`, PRB-85 `helper_hold`).
            for spec in item.keys.iter().filter(|spec| !spec.optional && !spec.no.is_empty()) {
                let given = row.values.iter().any(|(key, value)| *key == spec.name && !value.is_empty());
                if !given {
                    problems.push(format!("a no needs a value for {}", spec.name));
                }
            }
        }
    }
    if !problems.is_empty() {
        return State::Open(problems.join("; "));
    }
    match (answer, &item.fallback) {
        (true, _) => State::Yes,
        (false, Some(fallback)) => {
            if skipped {
                State::Skipped { reason: row.fallback.trim().to_string(), fallback: fallback.clone() }
            } else {
                State::Fallback(row.fallback.trim().to_string())
            }
        }
        (false, None) => {
            State::No(if row.fallback.trim().is_empty() { "answered no".to_string() } else { row.fallback.trim().to_string() })
        }
    }
}

fn switch_is_on(results: &Results, name: &str) -> bool {
    // A switch that says nothing yet counts as on: the item stays required (fail closed).
    !matches!(results.switches.get(name), Some(Some(false)))
}

fn state_of<'a>(states: &'a [(String, State)], id: &str) -> Option<&'a State> {
    states.iter().find(|(item_id, _)| item_id == id).map(|(_, state)| state)
}

/// Checks every item against the answers. `evidence_ok` says whether an evidence file exists and is not empty.
pub fn evaluate(items: &[Item], results: &Results, evidence_ok: &dyn Fn(&str) -> bool) -> Report {
    let mut states: Vec<(String, State)> = items
        .iter()
        .map(|item| {
            let required = item.only_with.as_deref().is_none_or(|switch| switch_is_on(results, switch));
            (item.id.clone(), base_state(item, results.rows.get(&item.id), required, evidence_ok))
        })
        .collect();

    // An item whose needed item is not a yes is not done, however well it was answered.
    loop {
        let mut changed = false;
        for (index, item) in items.iter().enumerate() {
            if !matches!(states[index].1, State::Yes | State::Fallback(_) | State::Skipped { .. }) {
                continue;
            }
            let unmet: Vec<&str> = item
                .needs
                .iter()
                .filter(|need| !matches!(state_of(&states, need.as_str()), Some(State::Yes)))
                .map(String::as_str)
                .collect();
            if !unmet.is_empty() {
                states[index].1 = State::Open(format!("needs {}", unmet.join(", ")));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut extras: Vec<(String, String)> = Vec::new();
    let used: BTreeSet<&str> = items.iter().filter_map(|item| item.only_with.as_deref()).collect();
    for switch in SWITCHES {
        if used.contains(switch) && !matches!(results.switches.get(switch), Some(Some(_))) {
            extras.push((switch.to_string(), "the switch must say yes or no".to_string()));
        }
    }
    if results.signed.is_none() {
        extras.push(("Signed".to_string(), "the Signed: line is not filled (name and date)".to_string()));
    }

    let mut problems = results.problems.clone();
    let known: BTreeSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
    for id in results.rows.keys() {
        if !known.contains(id.as_str()) {
            problems.push(format!("the row {id} is not an item of the checklist"));
        }
    }
    Report { states, extras, problems }
}

pub fn verdict(report: &Report) -> Verdict {
    if report.states.iter().any(|(_, state)| matches!(state, State::No(_))) {
        return Verdict::No;
    }
    let open = report.states.iter().any(|(_, state)| matches!(state, State::Open(_)))
        || !report.extras.is_empty()
        || !report.problems.is_empty();
    if open {
        return Verdict::Open;
    }
    // Ok, but never the plain yes while a fallback holds (R4): a no on a probe with a fallback and a skip reach ok only so.
    if report.states.iter().any(|(_, state)| matches!(state, State::Fallback(_) | State::Skipped { .. })) {
        Verdict::FallbackUsed
    } else {
        Verdict::Ok
    }
}

/// The lines `stage1-check` prints: the verdict first, then one line per reason. Every item that holds through its fallback
/// (a no taken, a skip) is listed with its fallback text or its reason, whatever the verdict is, so the release report names
/// each fallback that Stage 1 stands on (R4).
pub fn report_lines(report: &Report) -> Vec<String> {
    let mut no: Vec<&str> = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    let mut fallbacks: Vec<&str> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    for (id, state) in &report.states {
        match state {
            State::No(reason) => {
                no.push(id);
                reasons.push(format!("stage1-check: {id}: no \u{2014} {reason}"));
            }
            State::Open(reason) => {
                open.push(id);
                reasons.push(format!("stage1-check: {id}: {reason}"));
            }
            State::Fallback(taken) => {
                fallbacks.push(id);
                reasons.push(format!("stage1-check: {id}: fallback used \u{2014} {taken}"));
            }
            State::Skipped { reason, fallback } => {
                fallbacks.push(id);
                reasons.push(format!("stage1-check: {id}: skipped \u{2014} {reason} (fallback holds: {fallback})"));
            }
            State::Yes | State::NotRequired => {}
        }
    }
    for (name, reason) in &report.extras {
        open.push(name);
        reasons.push(format!("stage1-check: {name}: {reason}"));
    }
    for problem in &report.problems {
        reasons.push(format!("stage1-check: problem \u{2014} {problem}"));
    }
    if open.is_empty() && !report.problems.is_empty() {
        open.push("results");
    }
    let mut lines: Vec<String> = Vec::new();
    match verdict(report) {
        Verdict::Ok => lines.push("stage1-check: ok \u{2014} yes".to_string()),
        Verdict::FallbackUsed => lines.push(format!("stage1-check: ok \u{2014} fallback used \u{2014} {}", fallbacks.join(", "))),
        Verdict::No => {
            lines.push(format!("stage1-check: no \u{2014} {}", no.join(", ")));
            if !open.is_empty() {
                lines.push(format!("stage1-check: open \u{2014} {}", open.join(", ")));
            }
        }
        Verdict::Open => lines.push(format!("stage1-check: open \u{2014} {}", open.join(", "))),
    }
    lines.extend(reasons);
    lines
}

// ---------------------------------------------------------------------------------------------------------
// The facts
// ---------------------------------------------------------------------------------------------------------

/// What one signed item writes into `probe-facts.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct FactUpdate {
    pub id: String,
    pub status: &'static str,
    pub source: String,
    pub values: Vec<(String, String)>,
}

/// The facts the signed answers fill: a yes is `passed`, a no is `failed`, an open item and a skip write nothing (a skipped
/// probe gave no result, so its fact stays as it is). Without a signature it is an error: a fact comes only from a signed
/// result.
pub fn fact_updates(items: &[Item], results: &Results, report: &Report) -> Result<Vec<FactUpdate>, String> {
    let Some((_, date)) = &results.signed else {
        return Err("the results are not signed (the Signed: line), so no fact is written".to_string());
    };
    let mut updates = Vec::new();
    for item in items {
        let Some(fact) = &item.fact else {
            continue;
        };
        let status = match state_of(&report.states, &item.id) {
            Some(State::Yes) => "passed",
            Some(State::Fallback(_)) | Some(State::No(_)) => "failed",
            _ => continue,
        };
        let Some(row) = results.rows.get(&item.id) else {
            continue;
        };
        let mut values = Vec::new();
        for spec in &item.keys {
            if let Some((_, value)) = row.values.iter().find(|(key, _)| *key == spec.name) {
                if !value.is_empty() {
                    values.push((spec.name.clone(), value.clone()));
                }
            }
        }
        updates.push(FactUpdate {
            id: fact.clone(),
            status,
            source: format!("docs/probes/stage-1/results.md#{} (signed {date})", item.id),
            values,
        });
    }
    Ok(updates)
}

fn join_lines(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The end of the table that starts at line `start`: the next line that opens a table, or the end of the file.
fn table_end(lines: &[String], start: usize) -> usize {
    lines.iter().enumerate().skip(start + 1).find(|(_, line)| line.trim_start().starts_with('[')).map_or(lines.len(), |(i, _)| i)
}

fn is_key_line(line: &str, key: &str) -> bool {
    let t = line.trim_start();
    !t.starts_with('#') && t.split_once('=').is_some_and(|(name, _)| name.trim() == key)
}

fn is_any_key_line(line: &str) -> bool {
    let t = line.trim_start();
    !t.starts_with('#') && t.contains('=')
}

/// Sets `key = "value"` in the table at `start`: the line of the key is replaced, a missing key goes after the last key line.
fn set_key(lines: &mut Vec<String>, start: usize, key: &str, value: &str) {
    let end = table_end(lines, start);
    let new_line = format!("{key} = \"{value}\"");
    let existing = lines.iter().enumerate().take(end).skip(start + 1).find(|(_, line)| is_key_line(line, key)).map(|(i, _)| i);
    if let Some(at) = existing {
        lines[at] = new_line;
        return;
    }
    let last = lines.iter().enumerate().take(end).skip(start + 1).filter(|(_, line)| is_any_key_line(line)).map(|(i, _)| i).next_back();
    lines.insert(last.unwrap_or(start) + 1, new_line);
}

fn verify_facts(text: &str, updates: &[FactUpdate]) -> Result<(), String> {
    let table: toml::Table = text.parse().map_err(|e| format!("the facts file would not be valid TOML: {e}"))?;
    for update in updates {
        let Some(entry) = table.get(&update.id).and_then(toml::Value::as_table) else {
            return Err(format!("{} is not a table after the write", update.id));
        };
        let text_of = |key: &str| entry.get(key).and_then(toml::Value::as_str).map(str::to_string);
        if text_of("status").as_deref() != Some(update.status) {
            return Err(format!("{}: the status did not read back as {}", update.id, update.status));
        }
        if text_of("source").as_deref() != Some(update.source.as_str()) {
            return Err(format!("{}: the source did not read back as written", update.id));
        }
        for (key, value) in &update.values {
            if text_of(key).as_deref() != Some(value.as_str()) {
                return Err(format!("{}: {key} did not read back as written", update.id));
            }
        }
    }
    Ok(())
}

/// The facts file with the updates written into it; every comment and every other line stays as it was.
pub fn apply_facts(text: &str, updates: &[FactUpdate]) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for update in updates {
        if !plain_value(&update.source) || update.values.iter().any(|(_, value)| !plain_value(value)) {
            return Err(format!("{}: a value cannot be written between quotes", update.id));
        }
        let header = format!("[{}]", update.id);
        match lines.iter().position(|line| line.trim() == header) {
            Some(start) => {
                set_key(&mut lines, start, "status", update.status);
                set_key(&mut lines, start, "source", &update.source);
                for (key, value) in &update.values {
                    set_key(&mut lines, start, key, value);
                }
            }
            None => {
                if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("# {} (added by cargo xtask stage1-check --facts from the signed Stage 1 results)", update.id));
                lines.push(header);
                lines.push(format!("status = \"{}\"", update.status));
                lines.push(format!("source = \"{}\"", update.source));
                for (key, value) in &update.values {
                    lines.push(format!("{key} = \"{value}\""));
                }
            }
        }
    }
    let out = join_lines(&lines);
    verify_facts(&out, updates)?;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------
// Recording an answer
// ---------------------------------------------------------------------------------------------------------

/// One answer to record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub id: String,
    /// `yes`, `no` or `skip` (a `not run` is refused: it is no answer, R4).
    pub result: String,
    /// The evidence file names; `None` keeps the ones of the row.
    pub evidence: Option<Vec<String>>,
    /// Values to set; a key that the row has is replaced, the others stay. A skip carries none.
    pub values: Vec<(String, String)>,
    /// The last cell of the row: the fallback taken of a no, or the reason of a skip (`--reason`); `None` keeps the one of the
    /// row if that row is an answer of the same kind.
    pub fallback: Option<String>,
}

fn cell_ok(text: &str) -> bool {
    !text.contains('|') && !text.contains('\n') && !text.contains('\r')
}

fn join_values(values: &[(String, String)]) -> String {
    values.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join("; ")
}

/// The results text with the row of `record` replaced. A record that would not count (an evidence file that is missing, a
/// yes without its values, a no without its fallback, a skip without its reason or of a gate) is refused, so the file never
/// holds an answer the checker would call open for a reason the recorder could have seen. A `not run` is refused with the
/// way out: run the probe, or skip it with a reason (R4).
pub fn apply_record(md: &str, items: &[Item], record: &Record, evidence_ok: &dyn Fn(&str) -> bool) -> Result<String, String> {
    let item = items.iter().find(|item| item.id == record.id).ok_or_else(|| format!("{} is not an item of the checklist", record.id))?;
    let answer = record.result.trim().to_ascii_lowercase();
    if answer == "not run" {
        return Err(format!(
            "{id}: not run is no answer and blocks the checklist: run the probe (--result yes or no), or skip it with a reason \
(--result skip --reason <text>; a gate cannot be skipped)",
            id = item.id
        ));
    }
    if !matches!(answer.as_str(), "yes" | "no" | "skip") {
        return Err("--result must be yes, no or skip".to_string());
    }
    let skip = answer == "skip";
    let results = parse_results(md);
    let old = results.rows.get(&item.id);
    // A skip fills no fact and carries no values, so the values of an earlier answer do not stay. The last cell stays only
    // between answers of one kind: the reason of a skip is not the fallback taken of a later no, and the other way round.
    let mut values: Vec<(String, String)> = if skip { Vec::new() } else { old.map(|row| row.values.clone()).unwrap_or_default() };
    for (key, value) in &record.values {
        match values.iter_mut().find(|(existing, _)| existing == key) {
            Some(slot) => slot.1 = value.clone(),
            None => values.push((key.clone(), value.clone())),
        }
    }
    let evidence = record.evidence.clone().or_else(|| old.map(|row| row.evidence.clone())).unwrap_or_default();
    let fallback = record
        .fallback
        .clone()
        .or_else(|| old.filter(|row| (row.result == "skip") == skip).map(|row| row.fallback.clone()))
        .unwrap_or_default();
    let row = Row { id: item.id.clone(), result: answer, evidence, values, values_error: None, fallback };
    if !cell_ok(&row.fallback) || row.values.iter().any(|(_, value)| !cell_ok(value)) {
        return Err(format!("{}: a value or the fallback holds a bar or a line break", item.id));
    }
    // The values cell is read back as `key=value; key=value`: a value with a ';' or a '=' would read as another key or as a
    // bad cell, so the row would count as open although the record was taken.
    if row.values.iter().any(|(_, value)| value.contains(|c: char| matches!(c, ';' | '='))) {
        return Err(format!("{}: a value holds ';' or '=', which the values cell cannot keep apart", item.id));
    }
    if let State::Open(reason) = base_state(item, Some(&row), true, evidence_ok) {
        return Err(format!("{}: {reason}", item.id));
    }
    let mut lines: Vec<String> = md.lines().map(str::to_string).collect();
    let at = lines.iter().position(|line| line.trim().starts_with('|') && cells_of(line).first().is_some_and(|cell| *cell == item.id));
    let Some(at) = at else {
        return Err(format!("{} has no row in the results", item.id));
    };
    lines[at] = format!(
        "| {} | {} | {} | {} | {} |",
        row.id,
        row.result,
        row.evidence.join(", "),
        join_values(&row.values),
        row.fallback
    );
    Ok(join_lines(&lines))
}

/// The results text with the `Signed:` line filled.
pub fn apply_sign(md: &str, name: &str, date: &str) -> Result<String, String> {
    if name.trim().is_empty() || name.contains(|c: char| matches!(c, '<' | '>' | '|')) {
        return Err("--sign needs a name without < > or |".to_string());
    }
    if !looks_like_date(date) {
        return Err("--date must read YYYY-MM-DD".to_string());
    }
    let mut lines: Vec<String> = md.lines().map(str::to_string).collect();
    let Some(at) = lines.iter().position(|line| line.trim_start().starts_with("Signed:")) else {
        return Err("the results have no Signed: line".to_string());
    };
    lines[at] = format!("Signed: {} \u{2013} {date}", name.trim());
    Ok(join_lines(&lines))
}

// ---------------------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct Cli {
    checklist: String,
    results: String,
    evidence_dir: String,
    facts: Option<String>,
    record: Option<Record>,
    sign: Option<(String, String)>,
}

fn parse_cli(args: &[String]) -> Result<Cli, String> {
    let mut checklist = DEFAULT_CHECKLIST.to_string();
    let mut results = DEFAULT_RESULTS.to_string();
    let mut evidence_dir = DEFAULT_EVIDENCE.to_string();
    let mut facts: Option<String> = None;
    let mut record_id: Option<String> = None;
    let mut result: Option<String> = None;
    let mut evidence: Option<Vec<String>> = None;
    let mut values: Vec<(String, String)> = Vec::new();
    let mut fallback: Option<String> = None;
    let mut reason: Option<String> = None;
    let mut sign: Option<String> = None;
    let mut date: Option<String> = None;
    let mut it = args.iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--checklist" => checklist = it.next().ok_or("--checklist needs a file")?.clone(),
            "--results" => results = it.next().ok_or("--results needs a file")?.clone(),
            "--evidence-dir" => evidence_dir = it.next().ok_or("--evidence-dir needs a folder")?.clone(),
            "--facts" => {
                let takes_a_path = it.peek().is_some_and(|next| !next.starts_with("--"));
                let path = if takes_a_path { it.next().cloned() } else { None };
                facts = Some(path.unwrap_or_else(|| DEFAULT_FACTS.to_string()));
            }
            "--record" => record_id = Some(it.next().ok_or("--record needs an item id")?.clone()),
            "--result" => result = Some(it.next().ok_or("--result needs yes, no or skip")?.clone()),
            "--evidence" => evidence = Some(file_names(it.next().ok_or("--evidence needs file names")?)),
            "--value" => {
                let text = it.next().ok_or("--value needs key=value")?;
                let (key, value) = text.split_once('=').ok_or("--value must read key=value")?;
                values.push((key.trim().to_string(), value.trim().to_string()));
            }
            "--fallback" => fallback = Some(it.next().ok_or("--fallback needs a text")?.clone()),
            "--reason" => reason = Some(it.next().ok_or("--reason needs a text")?.clone()),
            "--sign" => sign = Some(it.next().ok_or("--sign needs a name")?.clone()),
            "--date" => date = Some(it.next().ok_or("--date needs YYYY-MM-DD")?.clone()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let record = match record_id {
        Some(id) => {
            let result = result.ok_or("--record needs --result yes, no or skip")?;
            // The last cell of the row has two names: the fallback taken of a no and the reason of a skip. They never meet.
            let skip = result.trim().eq_ignore_ascii_case("skip");
            if skip && fallback.is_some() {
                return Err("a skip takes --reason, not --fallback".to_string());
            }
            if !skip && reason.is_some() {
                return Err("--reason belongs to --result skip (a no takes --fallback)".to_string());
            }
            Some(Record { id, result, evidence, values, fallback: if skip { reason } else { fallback } })
        }
        None => {
            if result.is_some() || evidence.is_some() || !values.is_empty() || fallback.is_some() || reason.is_some() {
                return Err("--result, --evidence, --value, --fallback and --reason belong to --record".to_string());
            }
            None
        }
    };
    let sign = match (sign, date) {
        (Some(name), Some(date)) => Some((name, date)),
        (None, None) => None,
        _ => return Err("--sign and --date go together".to_string()),
    };
    if [facts.is_some(), record.is_some(), sign.is_some()].iter().filter(|set| **set).count() > 1 {
        return Err("one action at a time: --record, --sign or --facts".to_string());
    }
    Ok(Cli { checklist, results, evidence_dir, facts, record, sign })
}

fn read(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn print_problems(problems: &[String]) -> u8 {
    for problem in problems {
        println!("stage1-check: problem \u{2014} {problem}");
    }
    1
}

/// Whether `dir\name` is a file with content. The name has been checked by [`evidence_name_ok`] first, so it holds no folder.
pub fn evidence_file_ok(dir: &str, name: &str) -> bool {
    Path::new(dir).join(name).metadata().is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

fn write_back(path: &str, text: &str, done: &str) -> u8 {
    match fs::write(path, text) {
        Ok(()) => {
            println!("stage1-check: {done}");
            0
        }
        Err(e) => print_problems(&[format!("cannot write {path}: {e}")]),
    }
}

fn write_facts(path: &str, items: &[Item], results: &Results, report: &Report) -> u8 {
    let updates = match fact_updates(items, results, report) {
        Ok(updates) => updates,
        Err(message) => return print_problems(&[message]),
    };
    if updates.is_empty() {
        println!("stage1-check: facts \u{2014} nothing is signed yet, nothing written");
        return 0;
    }
    let text = match read(path) {
        Ok(text) => text,
        Err(message) => return print_problems(&[message]),
    };
    let written = match apply_facts(&text, &updates) {
        Ok(written) => written,
        Err(message) => return print_problems(&[message]),
    };
    let names: Vec<String> = updates.iter().map(|u| format!("{} ({})", u.id, u.status)).collect();
    write_back(path, &written, &format!("facts written \u{2014} {}", names.join(", ")))
}

/// `stage1-check`: exit 0 for `ok` (the plain yes, or `fallback used`), 1 for anything else (open, no, a bad file, a refused
/// record), 2 for a usage error.
pub fn run_cli(args: &[String]) -> u8 {
    let cli = match parse_cli(args) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("stage1-check: {message}");
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let checklist_text = match read(&cli.checklist) {
        Ok(text) => text,
        Err(message) => return print_problems(&[message]),
    };
    let (items, problems) = check_checklist(&checklist_text);
    if !problems.is_empty() {
        return print_problems(&problems);
    }
    let dir = cli.evidence_dir.clone();
    let exists = move |name: &str| evidence_file_ok(&dir, name);
    let results_text = match read(&cli.results) {
        Ok(text) => text,
        Err(message) => return print_problems(&[message]),
    };

    if let Some(record) = &cli.record {
        return match apply_record(&results_text, &items, record, &exists) {
            Ok(text) => write_back(&cli.results, &text, &format!("recorded {} \u{2014} {}", record.id, record.result.trim().to_ascii_lowercase())),
            Err(message) => print_problems(&[message]),
        };
    }
    if let Some((name, date)) = &cli.sign {
        return match apply_sign(&results_text, name, date) {
            Ok(text) => write_back(&cli.results, &text, &format!("signed \u{2014} {} {date}", name.trim())),
            Err(message) => print_problems(&[message]),
        };
    }

    let results = parse_results(&results_text);
    let report = evaluate(&items, &results, &exists);
    if let Some(facts_path) = &cli.facts {
        return write_facts(facts_path, &items, &results, &report);
    }
    for line in report_lines(&report) {
        println!("{line}");
    }
    if matches!(verdict(&report), Verdict::Ok | Verdict::FallbackUsed) { 0 } else { 1 }
}
