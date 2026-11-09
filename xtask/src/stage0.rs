// SPDX-License-Identifier: MIT
//! `cargo xtask stage0-check` (plan Tasks 18 and 19; MC-129, MC-134, MC-139, OPEN-23, L1337).
//!
//! ```text
//! cargo xtask stage0-check --checklist <file> [--template]        -> stage0-check: ok
//! cargo xtask stage0-check --results <file> --checklist <file>    -> stage0-check: ok — Stage 0 result: yes|no
//! ```
//!
//! The checklist (`docs/probes/stage-0/checklist.md`) holds one item per real-Windows check, in the form
//! `- [ ] <ID>.<n> <action> — expected: <pass text>` followed by a line
//! `Result: PASS | FAIL | NOT RUN (<reason>) — <value>`. In template mode every item is unticked and still
//! carries the placeholder; in signed mode every item is ticked and carries a filled result (a `NOT RUN` needs a
//! reason and is ticked too) and the signature line is filled. `results.md` cites checklist items (`PRB-10.x` cites every item of a
//! section) and the checker turns the signed checklist into the MC-129 decision. Exit codes: 0 ok, 1 problems,
//! 2 usage.

use std::collections::{BTreeMap, BTreeSet};

use crate::leak::{scan_text, Finding, ScanConfig};

/// The section IDs of the checklist, in the order of the plan.
pub const IDS: [&str; 22] = [
    "M", "S", "PRB-01", "PRB-02", "PRB-03", "PRB-05", "PRB-06", "PRB-10", "PRB-11", "PRB-20", "PRB-21", "PRB-31",
    "PRB-40", "PRB-40b", "PRB-40c", "PRB-40d", "PRB-40e", "PRB-49", "PRB-60", "PRB-61", "PRB-65", "R",
];

/// The six MC-129 conditions are numbered 1 to 6.
const CONDITIONS: usize = 6;
/// The disclaimer of L1326.
const DISCLAIMER: &str = "A yes to the probe build is not an approval of this design.";
/// The rows of the blocking-probe table outside the six conditions (MC-134 L1411, MC-128 L1322).
const BLOCKING_PROBES: [&str; 7] = ["PRB-05", "PRB-06", "PRB-11", "PRB-31", "PRB-60", "PRB-61", "PRB-65"];
/// The connection probes: never a "no", but a failure must name its fallback.
const CONNECTIONS: [&str; 4] = ["PRB-40b", "PRB-40c", "PRB-40d", "PRB-40e"];

/// How an item ended.
#[derive(Clone, PartialEq, Debug)]
pub enum ItemResult {
    Pass,
    Fail,
    NotRun(String),
}

/// One checklist item: its full ID (`PRB-60.1`, `S.1`, `M.1`), whether the owner ticked it and its result.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub id: String,
    pub ticked: bool,
    pub result: Option<ItemResult>,
}

// ---------------------------------------------------------------------------------------------------------
// Parsing the checklist
// ---------------------------------------------------------------------------------------------------------

/// What a `Result:` line says.
#[derive(Clone, PartialEq, Debug)]
enum Parsed {
    /// The template text `PASS | FAIL | NOT RUN (<reason>) — <value>`.
    Placeholder,
    Filled { result: ItemResult, value_ok: bool },
    /// Anything else.
    Invalid,
}

struct RawItem {
    id: String,
    ticked: bool,
    /// 1-based line number of the item.
    line: usize,
    /// The section heading the item stands under, when that heading is a section ID.
    section: Option<String>,
    has_expected: bool,
    /// `None`: the item has no result line.
    parsed: Option<Parsed>,
}

fn checkbox(line: &str) -> Option<(bool, &str)> {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("- [ ] ") {
        return Some((false, rest));
    }
    t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")).map(|rest| (true, rest))
}

/// The first word of a Markdown heading line; `None` for any other line.
fn heading_token(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if !t.starts_with('#') {
        return None;
    }
    Some(t.trim_start_matches('#').trim_start().split_whitespace().next().unwrap_or(""))
}

/// `rest` after `token` when `token` stands as a whole word at the start of `body`.
fn strip_token<'a>(body: &'a str, token: &str) -> Option<&'a str> {
    let rest = body.strip_prefix(token)?;
    match rest.chars().next() {
        Some(c) if c.is_alphanumeric() => None,
        _ => Some(rest),
    }
}

fn strip_dash(s: &str) -> &str {
    let s = s.trim_start();
    for dash in ['\u{2014}', '\u{2013}', '-'] {
        if let Some(rest) = s.strip_prefix(dash) {
            return rest.trim();
        }
    }
    s.trim()
}

/// The text after `Result:`.
fn parse_result_body(body: &str) -> Parsed {
    let body = body.trim();
    if body.contains("PASS | FAIL") {
        return Parsed::Placeholder;
    }
    let (result, rest) = if let Some(rest) = strip_token(body, "PASS") {
        (ItemResult::Pass, rest)
    } else if let Some(rest) = strip_token(body, "FAIL") {
        (ItemResult::Fail, rest)
    } else if let Some(rest) = strip_token(body, "NOT RUN") {
        let rest = rest.trim_start();
        let (reason, rest) = match rest.strip_prefix('(') {
            Some(inner) => match inner.split_once(')') {
                Some((reason, after)) => (reason.trim().to_string(), after),
                None => (String::new(), ""),
            },
            None => (String::new(), rest),
        };
        (ItemResult::NotRun(reason), rest)
    } else {
        return Parsed::Invalid;
    };
    let value = strip_dash(rest);
    Parsed::Filled { result, value_ok: !value.is_empty() && value != "<value>" }
}

fn parse_raw(md: &str) -> Vec<RawItem> {
    let lines: Vec<&str> = md.lines().collect();
    let mut items = Vec::new();
    let mut section: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(token) = heading_token(line) {
            section = if IDS.contains(&token) { Some(token.to_string()) } else { None };
            continue;
        }
        let Some((ticked, rest)) = checkbox(line) else {
            continue;
        };
        let id = rest.split_whitespace().next().unwrap_or("").to_string();
        let mut parsed = None;
        for next in &lines[i + 1..] {
            let t = next.trim();
            if t.is_empty() || heading_token(next).is_some() || checkbox(next).is_some() {
                break;
            }
            if let Some(body) = t.strip_prefix("Result:") {
                parsed = Some(parse_result_body(body));
                break;
            }
        }
        items.push(RawItem {
            id,
            ticked,
            line: i + 1,
            section: section.clone(),
            has_expected: rest.contains("expected:"),
            parsed,
        });
    }
    items
}

/// Every item of a checklist, in file order. An item whose result line is missing, still the placeholder or
/// not readable has `result: None`.
pub fn parse_items(md: &str) -> Vec<Item> {
    parse_raw(md)
        .into_iter()
        .map(|raw| Item {
            id: raw.id,
            ticked: raw.ticked,
            result: match raw.parsed {
                Some(Parsed::Filled { result, .. }) => Some(result),
                _ => None,
            },
        })
        .collect()
}

/// `PRB-10.x` cites every item of section `PRB-10`; `PRB-02.1` cites itself when the item exists. A cite of an
/// item that does not exist expands to nothing.
pub fn expand(cite: &str, items: &[Item]) -> Vec<String> {
    let cite = cite.trim();
    let Some((section, tail)) = cite.rsplit_once('.') else {
        return Vec::new();
    };
    if tail == "x" {
        let prefix = format!("{section}.");
        items.iter().filter(|i| i.id.starts_with(&prefix)).map(|i| i.id.clone()).collect()
    } else {
        items.iter().filter(|i| i.id == cite).map(|i| i.id.clone()).collect()
    }
}

/// `("PRB-60", 1)` for `PRB-60.1`; `None` unless the ID is a section ID, a dot and a positive number.
fn split_item_id(id: &str) -> Option<(&str, u32)> {
    let (section, number) = id.rsplit_once('.')?;
    if !IDS.contains(&section) || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u32 = number.parse().ok()?;
    if number == 0 {
        return None;
    }
    Some((section, number))
}

fn looks_like_date(s: &str) -> bool {
    s.len() == 10
        && s.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
}

/// `Signed: <handle> (owner's project handle) — date <YYYY-MM-DD> — ...` with the blanks filled in.
fn signature_filled(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("Signed:") else {
        return false;
    };
    let Some((handle, after)) = rest.split_once('(') else {
        return false;
    };
    let handle = handle.trim();
    if handle.is_empty() || handle.contains("__") {
        return false;
    }
    let Some((_, after)) = after.split_once(')') else {
        return false;
    };
    let Some((_, date_part)) = after.split_once("date") else {
        return false;
    };
    looks_like_date(date_part.trim_start().split_whitespace().next().unwrap_or(""))
}

/// Checks the shape of a checklist; one text per problem, empty when it is fine. `template == true` expects
/// every item unticked and still the placeholder; `false` expects a signed checklist.
pub fn check_checklist(md: &str, template: bool) -> Vec<String> {
    let mut problems = Vec::new();

    let sections: BTreeSet<&str> = md.lines().filter_map(heading_token).filter(|t| IDS.contains(t)).collect();
    for id in IDS {
        if !sections.contains(id) {
            problems.push(format!("section {id} is missing"));
        }
    }

    let raw = parse_raw(md);
    let mut counters: BTreeMap<String, u32> = BTreeMap::new();
    for item in &raw {
        let at = format!("line {}", item.line);
        match split_item_id(&item.id) {
            None => problems.push(format!("{at}: '{}' is not a full item ID (<ID>.<n>)", item.id)),
            Some((section, number)) => {
                if item.section.as_deref() != Some(section) {
                    problems.push(format!("{at}: item {} does not stand under its section {section}", item.id));
                }
                let counter = counters.entry(section.to_string()).or_insert(0);
                if number != *counter + 1 {
                    problems.push(format!(
                        "{at}: item {} breaks the numbering (expected {section}.{})",
                        item.id,
                        *counter + 1
                    ));
                }
                *counter = number;
            }
        }
        if !item.has_expected {
            problems.push(format!("{at}: item {} has no 'expected:' text", item.id));
        }
        match &item.parsed {
            None => problems.push(format!("{at}: item {} has no Result line", item.id)),
            Some(Parsed::Invalid) => problems.push(format!(
                "{at}: the Result line of {} is not PASS, FAIL or NOT RUN (<reason>)",
                item.id
            )),
            Some(_) => {}
        }
        if template {
            if item.ticked {
                problems.push(format!("{at}: item {} is ticked in the template", item.id));
            }
            if matches!(item.parsed, Some(Parsed::Filled { .. })) {
                problems.push(format!("{at}: the Result line of {} is filled in the template", item.id));
            }
        } else {
            match &item.parsed {
                Some(Parsed::Placeholder) => problems
                    .push(format!("{at}: the Result line of {} is still the placeholder", item.id)),
                Some(Parsed::Filled { result: ItemResult::NotRun(reason), .. }) => {
                    if reason.trim().is_empty() || reason == "<reason>" {
                        problems.push(format!("{at}: NOT RUN of {} needs a reason", item.id));
                    }
                    // The owner's tick covers every Result line, so a NOT RUN counts only when it is ticked too.
                    if !item.ticked {
                        problems.push(format!("{at}: item {} is NOT RUN but not ticked; the owner ticks every item", item.id));
                    }
                }
                Some(Parsed::Filled { value_ok, .. }) => {
                    if !item.ticked {
                        problems.push(format!("{at}: item {} has a result but is not ticked", item.id));
                    }
                    if !value_ok {
                        problems.push(format!("{at}: the Result line of {} has no value", item.id));
                    }
                }
                _ => {}
            }
        }
    }
    for id in IDS {
        if sections.contains(id) && !raw.iter().any(|i| i.section.as_deref() == Some(id)) {
            problems.push(format!("section {id} has no items"));
        }
    }

    match md.lines().map(str::trim).find(|l| l.starts_with("Signed:")) {
        None => problems.push("the Signed: line is missing".to_string()),
        Some(line) => {
            if !template && !signature_filled(line) {
                problems.push("the signature line is not filled in (handle and date)".to_string());
            }
        }
    }

    let probe = ScanConfig::for_test(&[], &[], "");
    if scan_text(&probe, md, "checklist").iter().any(|f| matches!(f, Finding::ProfilePath { .. })) {
        problems.push(
            "an expanded user-profile path appears in the checklist; write %USERPROFILE%, %LOCALAPPDATA% or %APPDATA%"
                .to_string(),
        );
    }
    problems
}

// ---------------------------------------------------------------------------------------------------------
// Task 19: the MC-129 decision
// ---------------------------------------------------------------------------------------------------------

/// The isolation tier recorded for Stage 1 (PRB-20, PRB-21); never changes the overall result (L1337).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tier {
    T1,
    T1b,
}

/// How a blocking probe outside the six conditions ended.
#[derive(Clone, PartialEq, Debug)]
pub enum Blocking {
    Pass,
    /// The failed probe's fallback that was taken.
    FallbackTaken(String),
    /// A failed probe without a fallback in the spec: the maintainer's recorded decision.
    MaintainerDecided(String),
}

/// What `check_results` returns when the results are consistent with the signed checklist.
#[derive(Clone, PartialEq, Debug)]
pub struct Decision {
    pub stage0: bool,
    pub tier: Tier,
    /// PRB-40b to PRB-40e: whether each passed (a failure names its fallback and is not a "no").
    pub connections: [(&'static str, bool); 4],
    pub blocking: Vec<(String, Blocking)>,
}

/// The cells of a Markdown table row; `None` for any other line.
fn table_cells(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('|')?;
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    Some(inner.split('|').map(|c| c.trim().to_string()).collect())
}

/// The table rows under the first heading that contains `heading_key`.
fn rows_under(md: &str, heading_key: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut inside = false;
    for line in md.lines() {
        if line.trim_start().starts_with('#') {
            inside = line.contains(heading_key);
            continue;
        }
        if inside {
            if let Some(cells) = table_cells(line) {
                rows.push(cells);
            }
        }
    }
    rows
}

/// The text after `prefix` on the first line that starts with it.
fn line_value<'a>(md: &'a str, prefix: &str) -> Option<&'a str> {
    md.lines().map(str::trim).find_map(|l| l.strip_prefix(prefix)).map(str::trim)
}

fn cell(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or("").trim()
}

fn yes_no(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Outcome {
    Pass,
    Fail,
    NotRun,
    Unset,
}

fn outcome(text: &str) -> Outcome {
    let t = text.trim().to_ascii_lowercase();
    if t.starts_with("pass") {
        Outcome::Pass
    } else if t.starts_with("fail") {
        Outcome::Fail
    } else if t.starts_with("not run") {
        Outcome::NotRun
    } else {
        Outcome::Unset
    }
}

/// `PRB-10.x`, `PRB-02.1`, `R.7`: a section ID, a dot and a number or `x`.
fn is_cite(token: &str) -> bool {
    let Some((section, tail)) = token.rsplit_once('.') else {
        return false;
    };
    let tail_ok = tail == "x" || (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()));
    tail_ok && !section.is_empty() && (IDS.contains(&section) || section.starts_with("PRB-"))
}

/// Every cite in a free-text evidence cell.
fn cites_in(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')'))
        .map(|t| t.trim_end_matches(['.', ':']))
        .filter(|t| is_cite(t))
        .map(str::to_string)
        .collect()
}

/// `maintainer: <decision> – <date>`: the decision text after the prefix.
fn strip_maintainer(text: &str) -> Option<&str> {
    let prefix = text.get(..11)?;
    if prefix.eq_ignore_ascii_case("maintainer:") { Some(text[11..].trim()) } else { None }
}

fn has_date(text: &str) -> bool {
    text.as_bytes().windows(10).any(|w| std::str::from_utf8(w).is_ok_and(looks_like_date))
}

/// A failed system-ConPTY cell of PRB-05 is covered when the matching bundle cell passed (OPEN-25). Within each
/// group of four cells (one flags value) the order is: system classic, system full-screen, bundle classic,
/// bundle full-screen, so system cell `n` pairs with bundle cell `n + 2`.
fn prb05_system_cell_covered(id: &str, items: &[Item]) -> bool {
    let Some(n) = id.strip_prefix("PRB-05.").and_then(|s| s.parse::<u32>().ok()) else {
        return false;
    };
    if n == 0 || n > 16 || (n - 1) % 4 >= 2 {
        return false;
    }
    let partner = format!("PRB-05.{}", n + 2);
    items.iter().any(|i| i.id == partner && i.ticked && i.result == Some(ItemResult::Pass))
}

/// Checks `results.md` against the signed checklist and returns the MC-129 decision.
///
/// - The result is `yes` only when all six conditions are `yes`; a condition marked `yes` needs every cited item
///   (through `expand`) to exist and to be ticked, either `PASS` or `NOT RUN (<reason>)`; any cited `FAIL` forces it to
///   `no`, except a failed system-ConPTY cell of PRB-05 whose matching bundle cell passed.
/// - Condition 1 must be `no` when any PRB-03 item failed (OPEN-23).
/// - The tier line is required and never changes the result (L1337); so is a failed PRB-40b to PRB-40e row, which
///   must name its fallback.
/// - Every row of the blocking-probe table is required. A row that did not pass must say which fallback was
///   taken; when the spec names none it needs `maintainer: <decision> – <date>`.
/// - Every evidence ID must exist in the signed checklist; the disclaimer line must be present.
pub fn check_results(results: &str, checklist: &str) -> Result<Decision, Vec<String>> {
    let mut problems = check_checklist(checklist, false);
    let items = parse_items(checklist);

    if !results.lines().any(|l| l.trim() == DISCLAIMER) {
        problems.push("the disclaimer line is missing: \"A yes to the probe build is not an approval of this design.\"".to_string());
    }

    let overall = match line_value(results, "Stage 0 result:") {
        Some("yes") => Some(true),
        Some("no") => Some(false),
        _ => {
            problems.push("'Stage 0 result:' must say yes or no".to_string());
            None
        }
    };

    let rows = rows_under(results, "MC-129 conditions");
    let mut conditions: [Option<bool>; CONDITIONS] = [None; CONDITIONS];
    for n in 1..=CONDITIONS {
        let key = n.to_string();
        let Some(row) = rows.iter().find(|r| r.first().is_some_and(|c| *c == key)) else {
            problems.push(format!("condition {n} is missing from the MC-129 table"));
            continue;
        };
        let value = yes_no(cell(row, 2));
        conditions[n - 1] = value;
        if value.is_none() {
            problems.push(format!("condition {n} needs a result: yes or no"));
        }
        let cites = cites_in(cell(row, 3));
        let mut expanded: Vec<String> = Vec::new();
        for cite in &cites {
            let ids = expand(cite, &items);
            if ids.is_empty() {
                problems.push(format!("condition {n} cites {cite}, which is not an item of the signed checklist"));
            }
            expanded.extend(ids);
        }
        if value == Some(true) {
            if cites.is_empty() {
                problems.push(format!("condition {n} is yes but cites no evidence"));
            }
            for id in &expanded {
                let Some(item) = items.iter().find(|i| i.id == *id) else {
                    continue;
                };
                match &item.result {
                    Some(ItemResult::Pass) if item.ticked => {}
                    Some(ItemResult::NotRun(reason)) if !reason.trim().is_empty() => {}
                    Some(ItemResult::Fail) => {
                        if !prb05_system_cell_covered(id, &items) {
                            problems.push(format!("condition {n} is yes but {id} failed"));
                        }
                    }
                    _ => problems.push(format!("condition {n} is yes but {id} is not ticked with a PASS")),
                }
            }
        }
    }
    if conditions[0] == Some(true)
        && items.iter().any(|i| i.id.starts_with("PRB-03.") && i.result == Some(ItemResult::Fail))
    {
        problems.push("condition 1 must be no: a PRB-03 item failed (OPEN-23)".to_string());
    }
    if overall == Some(true) && conditions.iter().any(|c| *c != Some(true)) {
        problems.push("the result is yes, but not all six conditions are yes".to_string());
    }

    let tier = match line_value(results, "Stage 1 isolation tier:") {
        Some(v) if v.starts_with("T1b") => Some(Tier::T1b),
        Some(v) if v.starts_with("T1") => Some(Tier::T1),
        _ => {
            problems.push("the 'Stage 1 isolation tier:' line must say T1 or T1b".to_string());
            None
        }
    };
    if let Some(evidence) = line_value(results, "Evidence:") {
        for cite in cites_in(evidence) {
            if expand(&cite, &items).is_empty() {
                problems.push(format!("the isolation evidence cites {cite}, which is not an item of the signed checklist"));
            }
        }
    }

    let rows = rows_under(results, "blocking probes");
    let mut blocking: Vec<(String, Blocking)> = Vec::new();
    for probe in BLOCKING_PROBES {
        let Some(row) = rows.iter().find(|r| r.first().is_some_and(|c| c.as_str() == probe)) else {
            problems.push(format!("the blocking-probe table has no row for {probe}"));
            continue;
        };
        let taken = cell(row, 3);
        match outcome(cell(row, 1)) {
            Outcome::Unset => problems.push(format!("{probe}: the blocking-probe row needs a result (PASS, FAIL or NOT RUN)")),
            Outcome::Pass => blocking.push((probe.to_string(), Blocking::Pass)),
            Outcome::Fail | Outcome::NotRun => {
                if taken.is_empty() {
                    problems.push(format!("{probe}: not passed; name the fallback taken or the maintainer's decision"));
                } else if let Some(decision) = strip_maintainer(taken) {
                    if has_date(decision) {
                        blocking.push((probe.to_string(), Blocking::MaintainerDecided(decision.to_string())));
                    } else {
                        problems.push(format!("{probe}: the maintainer's decision needs a date (maintainer: <decision> - <date>)"));
                    }
                } else if cell(row, 2).to_ascii_lowercase().starts_with("none") {
                    problems.push(format!(
                        "{probe}: the spec names no fallback, so a maintainer's decision is needed (maintainer: <decision> - <date>)"
                    ));
                } else {
                    blocking.push((probe.to_string(), Blocking::FallbackTaken(taken.to_string())));
                }
            }
        }
    }

    let rows = rows_under(results, "Connection probes");
    let mut connections: [(&'static str, bool); 4] = [("PRB-40b", false), ("PRB-40c", false), ("PRB-40d", false), ("PRB-40e", false)];
    for (slot, probe) in connections.iter_mut().zip(CONNECTIONS) {
        let Some(row) = rows.iter().find(|r| r.first().is_some_and(|c| c.as_str() == probe)) else {
            problems.push(format!("the connection-probe table has no row for {probe}"));
            continue;
        };
        match outcome(cell(row, 1)) {
            Outcome::Unset => problems.push(format!("{probe}: the connection-probe row needs a result (PASS, FAIL or NOT RUN)")),
            Outcome::Pass => slot.1 = true,
            Outcome::Fail | Outcome::NotRun => {
                slot.1 = false;
                if cell(row, 2).is_empty() {
                    problems.push(format!("{probe}: not passed; name the fallback taken"));
                }
            }
        }
    }

    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Decision {
        stage0: overall == Some(true),
        tier: tier.unwrap_or(Tier::T1),
        connections,
        blocking,
    })
}

// ---------------------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------------------

const USAGE: &str = "usage: cargo xtask stage0-check --checklist <file> [--template]\n       cargo xtask stage0-check --results <file> --checklist <file>";

#[derive(Debug, PartialEq)]
enum Cli {
    Checklist { checklist: String, template: bool },
    Results { results: String, checklist: String },
}

fn parse_cli(args: &[String]) -> Result<Cli, String> {
    let mut checklist: Option<String> = None;
    let mut results: Option<String> = None;
    let mut template = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--checklist" => checklist = Some(it.next().ok_or("--checklist needs a file")?.clone()),
            "--results" => results = Some(it.next().ok_or("--results needs a file")?.clone()),
            "--template" => template = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let checklist = checklist.ok_or("--checklist <file> is required")?;
    match results {
        Some(_) if template => Err("--template belongs to the checklist-only form".to_string()),
        Some(results) => Ok(Cli::Results { results, checklist }),
        None => Ok(Cli::Checklist { checklist, template }),
    }
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn report(problems: Vec<String>) -> u8 {
    for problem in problems {
        println!("stage0-check: {problem}");
    }
    1
}

/// `stage0-check`: exit 0 with `stage0-check: ok`, exit 1 with one line per problem, exit 2 on usage errors.
pub fn run_cli(args: &[String]) -> u8 {
    let cli = match parse_cli(args) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("stage0-check: {message}");
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match cli {
        Cli::Checklist { checklist, template } => {
            let md = match read(&checklist) {
                Ok(md) => md,
                Err(message) => return report(vec![message]),
            };
            let problems = check_checklist(&md, template);
            if problems.is_empty() {
                println!("stage0-check: ok");
                0
            } else {
                report(problems)
            }
        }
        Cli::Results { results, checklist } => {
            let (results_text, checklist_text) = match (read(&results), read(&checklist)) {
                (Ok(r), Ok(c)) => (r, c),
                (Err(m), _) | (_, Err(m)) => return report(vec![m]),
            };
            match check_results(&results_text, &checklist_text) {
                Ok(decision) => {
                    println!("stage0-check: ok \u{2014} Stage 0 result: {}", if decision.stage0 { "yes" } else { "no" });
                    0
                }
                Err(problems) => report(problems),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECKLIST: &str = include_str!("../../docs/probes/stage-0/checklist.md");

    const RESULTS: &str = include_str!("../../docs/probes/stage-0/results.md");
    const Y: bool = true;
    const N: bool = false;

    // --- checklist fixtures ---

    fn template() -> String {
        CHECKLIST.to_string()
    }

    fn template_without(id: &str) -> String {
        let mut out = String::new();
        let mut skipping = false;
        for line in CHECKLIST.lines() {
            if let Some(token) = heading_token(line) {
                skipping = token == id;
            }
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn template_with_text(text: &str) -> String {
        format!("{}\n{}\n", CHECKLIST.trim_end(), text)
    }

    fn template_with_item_missing_result() -> String {
        let mut out = String::new();
        let mut dropped = false;
        for line in CHECKLIST.lines() {
            if !dropped && line.trim_start().starts_with("Result:") {
                dropped = true;
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// The template with every item ticked and answered `PASS — ok`, the signature filled, and the given
    /// `(item ID, result text)` overrides.
    fn signed_with(overrides: &[(&str, &str)]) -> String {
        let mut out = String::new();
        let mut current = String::new();
        for line in CHECKLIST.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("- [ ] ") {
                current = rest.split_whitespace().next().unwrap_or("").to_string();
                out.push_str(&line.replacen("- [ ]", "- [x]", 1));
            } else if t.starts_with("Result:") {
                let body = overrides.iter().find(|(id, _)| *id == current).map_or("PASS \u{2014} ok", |(_, body)| *body);
                out.push_str(&format!("  Result: {body}"));
            } else if t.starts_with("Signed:") {
                out.push_str("Signed: tester (owner's project handle) \u{2014} date 2026-11-20 \u{2014} I read every Result line of this file. Every ticked item I worked through by hand on real Windows, or I read the assistant's recorded evidence for it and agree with it; every tick and every NOT RUN in this file is mine.");
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        out
    }

    fn signed() -> String {
        signed_with(&[])
    }

    fn signed_with_open_item() -> String {
        signed().replacen("- [x]", "- [ ]", 1)
    }

    fn unsigned() -> String {
        template()
    }

    fn signed_with_fail(id: &str) -> String {
        signed_with(&[(id, "FAIL \u{2014} did not work")])
    }

    // --- results fixtures ---

    struct Opts {
        overall: &'static str,
        conds: [bool; 6],
        /// (probe, result, fallback-taken cell)
        blocking: Vec<(&'static str, &'static str, &'static str)>,
        /// (probe, result, replacement of the fallback cell)
        conn: Vec<(&'static str, &'static str, Option<&'static str>)>,
        tier: Option<&'static str>,
        disclaimer: bool,
        drop_blocking: Option<&'static str>,
        evidence: Option<(usize, &'static str)>,
    }

    fn opts() -> Opts {
        Opts {
            overall: "yes",
            conds: [Y; 6],
            blocking: vec![],
            conn: vec![],
            tier: Some("T1"),
            disclaimer: true,
            drop_blocking: None,
            evidence: None,
        }
    }

    fn with_cell(mut cells: Vec<String>, index: usize, value: &str) -> String {
        cells[index] = value.to_string();
        format!("| {} |", cells.join(" | "))
    }

    fn row(o: &Opts, cells: Vec<String>, original: &str) -> Option<String> {
        let first = cells[0].clone();
        if let Ok(n) = first.parse::<usize>() {
            if (1..=6).contains(&n) {
                let mut s = with_cell(cells, 2, if o.conds[n - 1] { "yes" } else { "no" });
                if let Some((k, evidence)) = o.evidence {
                    if k == n {
                        s = with_cell(table_cells(&s).unwrap(), 3, evidence);
                    }
                }
                return Some(s);
            }
        }
        if BLOCKING_PROBES.contains(&first.as_str()) {
            if o.drop_blocking == Some(first.as_str()) {
                return None;
            }
            let (result, taken) = o
                .blocking
                .iter()
                .find(|(p, _, _)| *p == first)
                .map_or(("PASS", ""), |(_, r, t)| (*r, *t));
            let s = with_cell(cells, 1, result);
            return Some(with_cell(table_cells(&s).unwrap(), 3, taken));
        }
        if CONNECTIONS.contains(&first.as_str()) {
            let (result, fallback) = o
                .conn
                .iter()
                .find(|(p, _, _)| *p == first)
                .map_or(("PASS", None), |(_, r, f)| (*r, *f));
            let mut s = with_cell(cells, 1, result);
            if let Some(f) = fallback {
                s = with_cell(table_cells(&s).unwrap(), 2, f);
            }
            return Some(s);
        }
        Some(original.to_string())
    }

    fn build(o: &Opts) -> String {
        let mut out = String::new();
        for line in RESULTS.lines() {
            let t = line.trim();
            let rendered: Option<String> = if t.starts_with("Stage 0 result:") {
                Some(format!("Stage 0 result: {}", o.overall))
            } else if t == DISCLAIMER {
                o.disclaimer.then(|| line.to_string())
            } else if t.starts_with("Stage 1 isolation tier:") {
                o.tier.map(|v| format!("Stage 1 isolation tier: {v}"))
            } else if let Some(cells) = table_cells(t) {
                row(o, cells, line)
            } else {
                Some(line.to_string())
            };
            if let Some(r) = rendered {
                out.push_str(&r);
                out.push('\n');
            }
        }
        out
    }

    fn results_with(conds: &[bool; 6], overall: &'static str) -> String {
        let mut o = opts();
        o.conds = *conds;
        o.overall = overall;
        build(&o)
    }

    fn results_blocking(probe: &'static str, result: &'static str, taken: &'static str) -> String {
        let mut o = opts();
        o.blocking = vec![(probe, result, taken)];
        build(&o)
    }

    // --- Task 18 tests ---

    #[test]
    fn template_passes_shape_check() {
        assert_eq!(check_checklist(include_str!("../../docs/probes/stage-0/checklist.md"), true), Vec::<String>::new());
    }

    #[test]
    fn missing_probe_section_rejected() {
        assert!(!check_checklist(&template_without("PRB-05"), true).is_empty());
    }

    #[test]
    fn item_without_result_line_rejected() {
        assert!(!check_checklist(&template_with_item_missing_result(), true).is_empty());
    }

    #[test]
    fn item_ids_use_the_full_form() {
        assert!(!check_checklist(&template_with_text("- [ ] 60.1 x \u{2014} expected: y\nResult: PASS \u{2014} v"), true).is_empty());
        let items = parse_items(&template());
        for id in ["M.1", "S.1", "PRB-60.1", "PRB-40b.1", "R.1"] {
            assert!(items.iter().any(|i| i.id == id), "{id}");
        }
    }

    #[test]
    fn x_cites_every_item_of_a_section() {
        let items = parse_items(&template());
        let all = expand("PRB-10.x", &items);
        assert_eq!(all.len(), items.iter().filter(|i| i.id.starts_with("PRB-10.")).count());
        assert!(expand("PRB-02.9", &items).is_empty());
    }

    #[test]
    fn signed_checklist_requires_every_item_ticked_or_not_run_with_reason() {
        assert!(!check_checklist(&signed_with_open_item(), false).is_empty());
    }

    #[test]
    fn profile_path_in_checklist_rejected() {
        let p = ["C:", "Users", "sample-user"].join("\\");
        assert!(!check_checklist(&template_with_text(&p), true).is_empty());
    }

    #[test]
    fn signed_fixture_passes_the_signed_check() {
        assert_eq!(check_checklist(&signed(), false), Vec::<String>::new());
    }

    #[test]
    fn template_fails_the_signed_check_and_signed_fails_the_template_check() {
        assert!(!check_checklist(&template(), false).is_empty());
        assert!(!check_checklist(&signed(), true).is_empty());
    }

    #[test]
    fn every_section_has_items_and_numbering_runs_from_one() {
        let items = parse_items(&template());
        for id in IDS {
            let n = items.iter().filter(|i| i.id.starts_with(&format!("{id}."))).count();
            assert!(n >= 1, "{id}");
            assert!(items.iter().any(|i| i.id == format!("{id}.1")), "{id}.1");
        }
    }

    #[test]
    fn the_plan_item_counts_hold() {
        let items = parse_items(&template());
        let count = |id: &str| items.iter().filter(|i| i.id.starts_with(&format!("{id}."))).count();
        assert_eq!(count("M"), 14);
        assert_eq!(count("S"), 6);
        assert_eq!(count("PRB-05"), 16);
        assert_eq!(count("PRB-60"), 10);
        assert_eq!(count("R"), 7);
        assert!(items.iter().any(|i| i.id == "R.7"));
    }

    #[test]
    fn the_review_items_for_window_input_clipboard_and_hook_state_are_there() {
        let items = parse_items(&template());
        let count = |id: &str| items.iter().filter(|i| i.id.starts_with(&format!("{id}."))).count();
        // Appended to the plan's sections: PRB-01.10 to .13 (detached typing, Ctrl+Q and zoom keys, Ctrl+C and
        // Ctrl+Insert, paste keys), PRB-03.7 to .10 (detached AltGr, real AltGr against left Ctrl+Alt, the ISO
        // key, numpad digits), PRB-10.6 and .7 (state unknown until the run's own hook line, a kit without
        // hooks) and PRB-20.8 (long-path-lookup, low-label-outputs, the portable-pty line).
        assert_eq!(count("PRB-01"), 13);
        assert_eq!(count("PRB-03"), 10);
        assert_eq!(count("PRB-10"), 7);
        assert_eq!(count("PRB-20"), 8);
        for id in ["PRB-01.10", "PRB-01.13", "PRB-03.7", "PRB-03.10", "PRB-10.6", "PRB-10.7", "PRB-20.8"] {
            assert!(items.iter().any(|i| i.id == id), "{id}");
        }
    }

    #[test]
    fn the_dependency_graph_section_has_its_one_evidence_item() {
        // PRB-65 reads the build-machine evidence `prb65-cargo-tree.txt`.
        let items = parse_items(&template());
        assert!(items.iter().any(|i| i.id == "PRB-65.1"));
        assert_eq!(items.iter().filter(|i| i.id.starts_with("PRB-65.")).count(), 1);
    }

    #[test]
    fn every_windows_10_item_is_not_run_with_a_reason_and_ticked() {
        // Owner decision 2026-10-22, Windows 11 or newer (MC-68): the owner writes and ticks every Windows 10 item as
        // NOT RUN; the items stay.
        let mut ids: Vec<String> = Vec::new();
        ids.extend((8..=14).map(|n| format!("M.{n}")));
        ids.extend((4..=6).map(|n| format!("S.{n}")));
        ids.extend((6..=10).map(|n| format!("PRB-60.{n}")));
        ids.push("PRB-61.3".to_string());
        ids.extend((9..=16).map(|n| format!("PRB-05.{n}")));
        ids.extend((4..=6).map(|n| format!("R.{n}")));
        assert_eq!(ids.len(), 27);
        // The list is exactly the template's items for the Windows 10 PC, so a new or renamed item is noticed.
        let mut in_template: Vec<String> = CHECKLIST
            .lines()
            .filter_map(|line| checkbox(line).map(|(_, rest)| rest))
            .filter(|rest| rest.contains("Windows 10 PC"))
            .filter_map(|rest| rest.split_whitespace().next().map(str::to_string))
            .collect();
        in_template.sort();
        let mut listed = ids.clone();
        listed.sort();
        assert_eq!(in_template, listed);
        let overrides: Vec<(&str, &str)> =
            ids.iter().map(|id| (id.as_str(), "NOT RUN (owner decision 2026-10-22, Windows 11 or newer)")).collect();
        let checklist = signed_with(&overrides);
        assert!(check_checklist(&checklist, false).is_empty());
        assert!(check_results(&results_with(&[Y; 6], "yes"), &checklist).unwrap().stage0);
    }

    #[test]
    fn not_run_in_a_signed_checklist_needs_the_owners_tick() {
        let ticked = signed_with(&[("PRB-05.3", "NOT RUN (system cells pass) \u{2014} skipped")]);
        assert!(check_checklist(&ticked, false).is_empty());
        let unticked = ticked.replacen("- [x] PRB-05.3 ", "- [ ] PRB-05.3 ", 1);
        let problems = check_checklist(&unticked, false);
        assert!(problems.iter().any(|p| p.contains("PRB-05.3") && p.contains("NOT RUN but not ticked")), "{problems:?}");
        let problems = check_results(&results_with(&[Y; 6], "yes"), &unticked).unwrap_err();
        assert!(problems.iter().any(|p| p.contains("PRB-05.3") && p.contains("NOT RUN but not ticked")), "{problems:?}");
    }

    #[test]
    fn prb65_and_prb31_not_passed_need_a_dated_maintainer_line() {
        let problems = |probe: &'static str, taken: &'static str| {
            check_results(&results_blocking(probe, "NOT RUN", taken), &signed()).unwrap_err()
        };
        assert!(problems("PRB-65", "").iter().any(|p| p.contains("PRB-65: not passed")));
        // The placeholder of the results template carries no date.
        let placeholder = problems("PRB-65", "maintainer: <decision> - <date>");
        assert!(placeholder.iter().any(|p| p.contains("PRB-65: the maintainer's decision needs a date")), "{placeholder:?}");
        assert!(check_results(&results_blocking("PRB-65", "NOT RUN", "maintainer: evidence accepted - 2026-10-23"), &signed()).unwrap().stage0);
        // The spec names no fallback for the build of mxc-sdk, so free text is no decision for PRB-31 either.
        let free_text = problems("PRB-31", "the own thin layer");
        assert!(free_text.iter().any(|p| p.contains("PRB-31: the spec names no fallback")), "{free_text:?}");
        let decided = check_results(&results_blocking("PRB-31", "NOT RUN", "maintainer: rc.exe question open - 2026-10-23"), &signed()).unwrap();
        assert!(decided.blocking.contains(&("PRB-31".into(), Blocking::MaintainerDecided("rc.exe question open - 2026-10-23".into()))));
    }

    #[test]
    fn signed_checklist_needs_a_filled_signature() {
        let blank = signed().replace("Signed: tester", "Signed: ________");
        assert!(!check_checklist(&blank, false).is_empty());
        let no_date = signed().replace("date 2026-11-20", "date ________");
        assert!(!check_checklist(&no_date, false).is_empty());
        let no_line = signed().lines().filter(|l| !l.starts_with("Signed:")).collect::<Vec<_>>().join("\n");
        assert!(!check_checklist(&no_line, false).is_empty());
    }

    #[test]
    fn not_run_needs_a_reason_and_a_pass_needs_a_value() {
        let no_reason = signed_with(&[("PRB-05.3", "NOT RUN \u{2014} skipped")]);
        assert!(!check_checklist(&no_reason, false).is_empty());
        let placeholder_reason = signed_with(&[("PRB-05.3", "NOT RUN (<reason>) \u{2014} skipped")]);
        assert!(!check_checklist(&placeholder_reason, false).is_empty());
        let fine = signed_with(&[("PRB-05.3", "NOT RUN (system cells pass) \u{2014} skipped")]);
        assert!(check_checklist(&fine, false).is_empty());
        let no_value = signed_with(&[("M.1", "PASS")]);
        assert!(!check_checklist(&no_value, false).is_empty());
        let junk = signed_with(&[("M.1", "MAYBE \u{2014} x")]);
        assert!(!check_checklist(&junk, false).is_empty());
    }

    #[test]
    fn filled_result_in_the_template_is_rejected() {
        let filled = template().replacen("Result: PASS | FAIL | NOT RUN (<reason>) \u{2014} <value>", "Result: PASS \u{2014} ok", 1);
        assert!(!check_checklist(&filled, true).is_empty());
        let ticked = template().replacen("\n- [ ] M.1 ", "\n- [x] M.1 ", 1);
        assert!(!check_checklist(&ticked, true).is_empty());
    }

    #[test]
    fn item_must_stand_under_its_section_with_continuous_numbers() {
        let moved = template_with_text("## Signature\n\n- [ ] M.15 x \u{2014} expected: y\nResult: PASS | FAIL | NOT RUN (<reason>) \u{2014} <value>");
        assert!(!check_checklist(&moved, true).is_empty());
        let gap = template().replacen("- [ ] M.2 ", "- [ ] M.3 ", 1);
        assert!(!check_checklist(&gap, true).is_empty());
    }

    #[test]
    fn item_without_expected_text_is_rejected() {
        let text = template().replacen(" \u{2014} expected:", " \u{2014} wanted:", 1);
        assert!(!check_checklist(&text, true).is_empty());
    }

    #[test]
    fn result_lines_parse_into_results() {
        let items = parse_items(&signed_with(&[
            ("M.1", "PASS \u{2014} ok"),
            ("M.2", "FAIL \u{2014} no"),
            ("M.3", "NOT RUN (no second monitor) \u{2014} skipped"),
        ]));
        let by = |id: &str| items.iter().find(|i| i.id == id).unwrap();
        assert_eq!(by("M.1").result, Some(ItemResult::Pass));
        assert_eq!(by("M.2").result, Some(ItemResult::Fail));
        assert_eq!(by("M.3").result, Some(ItemResult::NotRun("no second monitor".to_string())));
        assert!(by("M.1").ticked);
        let items = parse_items(&template());
        assert!(items.iter().all(|i| !i.ticked && i.result.is_none()));
    }

    #[test]
    fn expand_of_a_single_item() {
        let items = parse_items(&template());
        assert_eq!(expand("PRB-02.1", &items), vec!["PRB-02.1".to_string()]);
        assert!(expand("PRB-02.99", &items).is_empty());
        assert!(expand("nonsense", &items).is_empty());
        assert!(!expand("PRB-40b.x", &items).iter().any(|id| id.starts_with("PRB-40.")));
    }

    // --- Task 19 tests ---

    #[test]
    fn overall_yes_requires_all_six_yes() {
        assert!(check_results(&results_with(&[Y, Y, Y, N, Y, Y], "yes"), &signed()).is_err());
        assert!(check_results(&results_with(&[Y; 6], "yes"), &signed()).unwrap().stage0);
    }

    #[test]
    fn overall_no_allowed_with_any_no() {
        assert!(!check_results(&results_with(&[Y, N, Y, Y, Y, Y], "no"), &signed()).unwrap().stage0);
    }

    #[test]
    fn prb03_fail_forces_condition_1_no() {
        let mut o = opts();
        o.evidence = Some((1, "PRB-01.x, PRB-05.x")); // does not cite PRB-03 itself
        let text = build(&o);
        assert!(check_results(&text, &signed_with_fail("PRB-03.2")).is_err());
        // With condition 1 answered no, the same checklist is consistent.
        let mut o = opts();
        o.conds = [N, Y, Y, Y, Y, Y];
        o.overall = "no";
        assert!(!check_results(&build(&o), &signed_with_fail("PRB-03.2")).unwrap().stage0);
    }

    #[test]
    fn cited_fail_forces_its_condition_no() {
        assert!(check_results(&results_with(&[Y; 6], "yes"), &signed_with_fail("PRB-10.5")).is_err());
    }

    #[test]
    fn system_cell_fail_covered_by_passing_bundle_cell_keeps_condition_1() {
        let checklist = signed_with(&[("PRB-05.1", "FAIL \u{2014} Esc did not interrupt"), ("PRB-05.3", "PASS \u{2014} ok")]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &checklist).unwrap().stage0);
    }

    #[test]
    fn system_cell_fail_without_a_passing_bundle_cell_is_not_covered() {
        let not_run = signed_with(&[
            ("PRB-05.1", "FAIL \u{2014} Esc did not interrupt"),
            ("PRB-05.3", "NOT RUN (no yes) \u{2014} package not approved"),
        ]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &not_run).is_err());
        // PRB-05.4 passing does not cover PRB-05.1: the partner of 05.1 is 05.3.
        let wrong_partner = signed_with(&[
            ("PRB-05.1", "FAIL \u{2014} x"),
            ("PRB-05.3", "NOT RUN (no yes) \u{2014} package not approved"),
            ("PRB-05.4", "PASS \u{2014} ok"),
        ]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &wrong_partner).is_err());
        let bundle_failed = signed_with(&[("PRB-05.1", "FAIL \u{2014} x"), ("PRB-05.3", "FAIL \u{2014} y")]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &bundle_failed).is_err());
        let windows10 = signed_with(&[("PRB-05.10", "FAIL \u{2014} x"), ("PRB-05.12", "PASS \u{2014} ok")]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &windows10).unwrap().stage0);
        let bundle_cell_failing_alone = signed_with(&[("PRB-05.3", "FAIL \u{2014} x")]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &bundle_cell_failing_alone).is_err());
    }

    #[test]
    fn not_run_with_reason_is_accepted_as_cited_evidence() {
        let checklist = signed_with(&[("PRB-10.4", "NOT RUN (no prompt tool on this PC) \u{2014} skipped")]);
        assert!(check_results(&results_with(&[Y; 6], "yes"), &checklist).unwrap().stage0);
    }

    #[test]
    fn blocking_probe_fail_without_named_fallback_blocks_yes() {
        assert!(check_results(&results_blocking("PRB-06", "FAIL", ""), &signed()).is_err());
        assert!(check_results(&results_blocking("PRB-06", "FAIL", "maintainer: accepted \u{2013} 2026-11-20"), &signed()).unwrap().stage0);
        // The decision needs a date.
        assert!(check_results(&results_blocking("PRB-06", "FAIL", "maintainer: accepted"), &signed()).is_err());
        // Free text is not a decision when the spec names no fallback.
        assert!(check_results(&results_blocking("PRB-06", "FAIL", "we will see"), &signed()).is_err());
    }

    #[test]
    fn blocking_probe_fail_with_named_fallback_records_it() {
        assert!(check_results(&results_blocking("PRB-61", "FAIL", ""), &signed()).is_err());
        let d = check_results(&results_blocking("PRB-61", "FAIL", "rung 1"), &signed()).unwrap();
        assert!(d.stage0 && d.blocking.contains(&("PRB-61".into(), Blocking::FallbackTaken("rung 1".into()))));
    }

    #[test]
    fn maintainer_decision_is_recorded() {
        let d = check_results(&results_blocking("PRB-06", "FAIL", "maintainer: accepted \u{2013} 2026-11-20"), &signed()).unwrap();
        assert!(d.blocking.contains(&("PRB-06".into(), Blocking::MaintainerDecided("accepted \u{2013} 2026-11-20".into()))));
    }

    #[test]
    fn blocking_row_with_no_result_is_rejected() {
        assert!(check_results(&results_blocking("PRB-11", "", ""), &signed()).is_err());
    }

    #[test]
    fn every_blocking_row_required() {
        let mut o = opts();
        o.drop_blocking = Some("PRB-65");
        assert!(check_results(&build(&o), &signed()).is_err());
    }

    #[test]
    fn default_fixture_gives_the_expected_decision() {
        let d = check_results(&build(&opts()), &signed()).unwrap();
        assert!(d.stage0);
        assert_eq!(d.tier, Tier::T1);
        assert_eq!(d.connections, [("PRB-40b", true), ("PRB-40c", true), ("PRB-40d", true), ("PRB-40e", true)]);
        assert_eq!(d.blocking.len(), 7);
        assert!(d.blocking.iter().all(|(_, b)| *b == Blocking::Pass));
    }

    #[test]
    fn isolation_failure_is_not_a_no() {
        let mut o = opts();
        o.tier = Some("T1b (labelled weaker)");
        let d = check_results(&build(&o), &signed()).unwrap();
        assert!(d.stage0);
        assert_eq!(d.tier, Tier::T1b);
    }

    #[test]
    fn tier_line_required() {
        let mut o = opts();
        o.tier = None;
        assert!(check_results(&build(&o), &signed()).is_err());
        o.tier = Some("<T1 | T1b (labelled weaker)>");
        assert!(check_results(&build(&o), &signed()).is_err());
    }

    #[test]
    fn failed_connection_probe_needs_its_fallback() {
        let mut o = opts();
        o.conn = vec![("PRB-40b", "FAIL", Some(""))];
        assert!(check_results(&build(&o), &signed()).is_err());
        o.conn = vec![("PRB-40b", "FAIL", Some("role card lists them as from the account (MC-152)"))];
        let d = check_results(&build(&o), &signed()).unwrap();
        assert!(d.stage0);
        assert_eq!(d.connections[0], ("PRB-40b", false));
        assert_eq!(d.connections[1], ("PRB-40c", true));
    }

    #[test]
    fn evidence_ids_must_exist_in_signed_checklist() {
        let mut o = opts();
        o.evidence = Some((2, "PRB-02.9"));
        assert!(check_results(&build(&o), &signed()).is_err());
        // Also for a condition answered no.
        let mut o = opts();
        o.conds = [Y, N, Y, Y, Y, Y];
        o.overall = "no";
        o.evidence = Some((2, "PRB-02.9"));
        assert!(check_results(&build(&o), &signed()).is_err());
    }

    #[test]
    fn unsigned_checklist_rejected() {
        assert!(check_results(&results_with(&[Y; 6], "yes"), &unsigned()).is_err());
    }

    #[test]
    fn disclaimer_line_required() {
        let mut o = opts();
        o.disclaimer = false;
        assert!(check_results(&build(&o), &signed()).is_err());
    }

    #[test]
    fn unfilled_results_template_is_rejected() {
        let problems = check_results(RESULTS, &signed()).unwrap_err();
        assert!(problems.iter().any(|p| p.contains("Stage 0 result")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("condition 1")), "{problems:?}");
    }

    #[test]
    fn result_must_say_yes_or_no() {
        let mut o = opts();
        o.overall = "<yes|no>";
        assert!(check_results(&build(&o), &signed()).is_err());
    }

    #[test]
    fn yes_condition_without_evidence_is_rejected() {
        let mut o = opts();
        o.evidence = Some((5, ""));
        assert!(check_results(&build(&o), &signed()).is_err());
    }

    #[test]
    fn cites_are_found_in_free_text() {
        assert_eq!(cites_in("PRB-01.x, PRB-03.x, PRB-05.x"), vec!["PRB-01.x", "PRB-03.x", "PRB-05.x"]);
        assert_eq!(cites_in("PRB-20.x, PRB-21.2. On failure: see 10.4 and (MC-51)"), vec!["PRB-20.x", "PRB-21.2"]);
        assert_eq!(cites_in("R.7 and M.1; S.x"), vec!["R.7", "M.1", "S.x"]);
        assert!(cites_in("PRB-02, version 0.37").is_empty());
    }

    #[test]
    fn table_rows_split_into_trimmed_cells() {
        assert_eq!(table_cells("| 1 | a b | | PRB-01.x |"), Some(vec!["1".into(), "a b".into(), "".into(), "PRB-01.x".into()]));
        assert_eq!(table_cells("not a row"), None);
    }

    #[test]
    fn maintainer_prefix_and_dates() {
        assert_eq!(strip_maintainer("Maintainer: ok 2026-11-20"), Some("ok 2026-11-20"));
        assert_eq!(strip_maintainer("rung 1"), None);
        assert!(has_date("accepted \u{2013} 2026-11-20"));
        assert!(!has_date("accepted 2026-11"));
        assert!(!has_date("20261120 and 2026-1-1"));
    }

    // --- command line ---

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_line_forms() {
        assert_eq!(
            parse_cli(&args(&["--checklist", "c.md", "--template"])),
            Ok(Cli::Checklist { checklist: "c.md".into(), template: true })
        );
        assert_eq!(
            parse_cli(&args(&["--checklist", "c.md"])),
            Ok(Cli::Checklist { checklist: "c.md".into(), template: false })
        );
        assert_eq!(
            parse_cli(&args(&["--results", "r.md", "--checklist", "c.md"])),
            Ok(Cli::Results { results: "r.md".into(), checklist: "c.md".into() })
        );
        assert!(parse_cli(&args(&[])).is_err());
        assert!(parse_cli(&args(&["--results", "r.md"])).is_err());
        assert!(parse_cli(&args(&["--results", "r.md", "--checklist", "c.md", "--template"])).is_err());
        assert!(parse_cli(&args(&["--checklist"])).is_err());
        assert!(parse_cli(&args(&["--bogus"])).is_err());
        assert_eq!(run_cli(&args(&["--bogus"])), 2);
    }
}
