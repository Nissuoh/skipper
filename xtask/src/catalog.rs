// SPDX-License-Identifier: MIT
//! The model catalogue, built offline (plan 11 a6, MODEL-3; MC-516.4 build part, MC-517.1, MC-517.2).
//!
//! [`build`] is pure: three texts in, one text out, no clock, no file, no network. The texts are
//!
//! - the answer of Anthropic's `GET /v1/models` (JSON): which models exist, which effort levels each one offers
//!   (`capabilities.effort.<level>.supported`) and its status (`status`, and `retire_on` when the answer names one);
//! - the benchmark table of the Epoch AI hub (CSV, RFC 4180, read here by hand): scores that become priors, a starting
//!   hint per category and never the verdict;
//! - the hand table `xtask/data/model-hand.toml`: what no source states - the name a start line carries, the family, the
//!   rank, the default effort, who pays, the lowest program version, which Anthropic ids and which Epoch names belong to
//!   the model, and which Epoch benchmark counts for which category.
//!
//! The result is the text of `model-catalog.toml` in the format `skipper_core::profile::catalog::parse_catalog` reads
//! (xtask does not depend on skipper-core here, so the format is written locally and the candidate is checked by that
//! parser in a test of the core). The text is deterministic: models sorted by rank, then alias; priors in the fixed
//! category order; every string quoted the way the core's `toml_string` quotes it.
//!
//! Fail closed: any problem - a model of Anthropic that the hand table does not know, a model without effort levels or
//! status, a score that is no number, a hand table with an unknown key - refuses the whole build and returns every
//! problem found; nothing partial is produced. No source other than Anthropic's model list and the Epoch table is read
//! (MC-517.2), and the Epoch line of attribution (MC-517.1) is written whenever a prior from it is.
//!
//! Where the fetch and the publishing of the file live: MODEL-15. This module has no network code.

use std::collections::BTreeMap;

/// The largest catalogue text the program reads (`profile::catalog::CATALOG_CAP`): a bigger result is refused here.
pub const CATALOG_CAP: usize = 256 * 1024;

/// The longest plain name of the catalogue (`launch::MAX_VALUE_LEN`).
const MAX_NAME: usize = 64;

/// The effort words in their one fixed order.
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The categories a prior may name, in the order they are written.
const CATEGORIES: [&str; 4] = ["coding", "agentic_terminal", "reasoning", "research"];

/// The status words of the program.
const STATUSES: [&str; 4] = ["active", "deprecated", "retiring", "retired"];

/// The source name of a prior.
pub const EPOCH_SOURCE: &str = "Epoch AI";

/// The attribution lines (MC-517.1). The Epoch line is written only with at least one prior from that source.
pub const ANTHROPIC_LINE: &str = "Model metadata: Anthropic";
/// The Epoch line is the whole credit that CC BY 4.0 asks for (section 3(a); closing review SP-4, finding [3]): the creator
/// and the work, the link to the data (the address the fetch reads, `EPOCH_URL` of the network module, which this module
/// may not link: a test keeps the two equal), the licence with its link, and the note that the data was changed. The program's
/// catalogue reader refuses a line of more than 200 characters, and with it the whole catalogue: a test holds this one to
/// that. One plain line, no quote, no backslash.
pub const EPOCH_LINE: &str = concat!(
    "Epoch AI, Capabilities & benchmarking (https://epoch.ai/data/benchmark_data/benchmarks_runs.csv), ",
    "CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Changed by Skipper: selected and converted."
);

/// The columns the Epoch table must have.
const CSV_MODEL: &str = "model";
const CSV_BENCHMARK: &str = "benchmark";
const CSV_SCORE: &str = "score";

const HAND_TOP_KEYS: [&str; 3] = ["epoch_scale", "categories", "model"];
const HAND_MODEL_KEYS: [&str; 9] =
    ["alias", "id", "family", "rank", "default_effort", "billing", "min_program", "anthropic", "epoch"];

// ---------------------------------------------------------------------------------------------------------------
// CSV, RFC 4180
// ---------------------------------------------------------------------------------------------------------------

/// The records of a CSV text (RFC 4180): fields split by `,`, records by `\n` or `\r\n`, a field in double quotes may hold
/// commas, line breaks and `""` for one quote. A byte order mark at the start and the line break after the last record are
/// ignored, and an empty line is no record. A text that is not valid CSV (an unclosed quote, a quote inside an unquoted
/// field, text after a closing quote) has no records here; [`build`] reads it strictly and names the problem.
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    read_csv(text).unwrap_or_default()
}

/// [`csv_rows`] with the reason a text is refused.
fn read_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut line = 1usize;
    // True while nothing of the current field has been read: only then may a quote open a quoted field, and a line
    // break at the start of a record is an empty line.
    let mut at_start = true;
    while let Some(c) = chars.next() {
        match c {
            '"' if at_start => {
                at_start = false;
                loop {
                    match chars.next() {
                        None => return Err(format!("line {line}: a quote is never closed")),
                        Some('"') => {
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                field.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(other) => {
                            if other == '\n' {
                                line += 1;
                            }
                            field.push(other);
                        }
                    }
                }
                if !matches!(chars.peek(), None | Some(',') | Some('\n') | Some('\r')) {
                    return Err(format!("line {line}: text follows a closing quote"));
                }
            }
            '"' => return Err(format!("line {line}: a quote inside an unquoted field")),
            ',' => {
                row.push(std::mem::take(&mut field));
                at_start = true;
            }
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    return Err(format!("line {line}: a carriage return without a line feed"));
                }
            }
            '\n' => {
                if !(at_start && row.is_empty() && field.is_empty()) {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                at_start = true;
                line += 1;
            }
            other => {
                at_start = false;
                field.push(other);
            }
        }
    }
    if !(at_start && row.is_empty() && field.is_empty()) {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------------------------------------------
// The hand table
// ---------------------------------------------------------------------------------------------------------------

/// One `[[model]]` row of the hand table.
struct HandModel {
    alias: String,
    id: String,
    family: String,
    rank: u32,
    default_effort: String,
    billing: String,
    min_program: Option<String>,
    /// The Anthropic ids this row stands for: the id itself or the id followed by `-` and a date of eight digits.
    anthropic: Vec<String>,
    /// The names this model has in the Epoch table.
    epoch: Vec<String>,
}

struct Hand {
    /// The factor that turns an Epoch score into a percentage (`1` for percentages, `100` for fractions).
    epoch_scale: f64,
    /// Epoch benchmark name to category.
    categories: BTreeMap<String, String>,
    models: Vec<HandModel>,
}

/// A text that is safe to show in a message: control characters replaced, cut at 80 characters.
fn shown(text: &str) -> String {
    let mut out: String = text.chars().take(80).map(|c| if c.is_control() { '?' } else { c }).collect();
    if text.chars().count() > 80 {
        out.push_str("...");
    }
    out
}

/// A plain name: the program's `is_plain_value` (an ASCII letter or digit first, then letters, digits and `. _ : [ ] -`,
/// at most 64 characters), so that no name reaches a start line that could read as an argument.
fn is_plain_name(text: &str) -> bool {
    let mut chars = text.chars();
    text.len() <= MAX_NAME
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '[' | ']' | '-'))
}

/// `YYYY-MM-DD` in plain digits.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10 && bytes.iter().enumerate().all(|(i, b)| if i == 4 || i == 7 { *b == b'-' } else { b.is_ascii_digit() })
}

/// `major.minor.patch` in plain digits, no leading zero (the way `Version` writes itself back).
fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn hand_text(table: &toml::Table, at: &str, key: &str, problems: &mut Vec<String>) -> Option<String> {
    match table.get(key) {
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => {
            problems.push(format!("hand table: {at}{key} is not a text"));
            None
        }
        None => {
            problems.push(format!("hand table: {at}{key} is missing"));
            None
        }
    }
}

fn hand_list(table: &toml::Table, at: &str, key: &str, required: bool, problems: &mut Vec<String>) -> Vec<String> {
    match table.get(key) {
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(s) if !s.is_empty() => out.push(s.to_string()),
                    _ => problems.push(format!("hand table: {at}{key} holds something that is no text")),
                }
            }
            if required && out.is_empty() {
                problems.push(format!("hand table: {at}{key} is empty"));
            }
            out
        }
        Some(_) => {
            problems.push(format!("hand table: {at}{key} is not a list"));
            Vec::new()
        }
        None => {
            if required {
                problems.push(format!("hand table: {at}{key} is missing"));
            }
            Vec::new()
        }
    }
}

fn read_hand(text: &str, problems: &mut Vec<String>) -> Option<Hand> {
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(_) => {
            problems.push("hand table: not valid TOML".to_string());
            return None;
        }
    };
    let before = problems.len();
    for key in table.keys() {
        if !HAND_TOP_KEYS.contains(&key.as_str()) {
            problems.push(format!("hand table: unknown key {}", shown(key)));
        }
    }
    let epoch_scale = match table.get("epoch_scale") {
        Some(toml::Value::Integer(n)) if *n > 0 && *n <= 1000 => *n as f64,
        Some(toml::Value::Float(f)) if f.is_finite() && *f > 0.0 && *f <= 1000.0 => *f,
        Some(_) => {
            problems.push("hand table: epoch_scale is a number above 0 and up to 1000".to_string());
            1.0
        }
        None => {
            problems.push("hand table: epoch_scale is missing".to_string());
            1.0
        }
    };
    let mut categories = BTreeMap::new();
    match table.get("categories") {
        Some(toml::Value::Table(rows)) => {
            for (benchmark, value) in rows {
                match value.as_str() {
                    Some(category) if CATEGORIES.contains(&category) => {
                        categories.insert(benchmark.clone(), category.to_string());
                    }
                    _ => problems.push(format!("hand table: category of {} is not one of the four", shown(benchmark))),
                }
            }
        }
        Some(_) => problems.push("hand table: categories is not a table".to_string()),
        None => problems.push("hand table: categories is missing".to_string()),
    }
    let mut models: Vec<HandModel> = Vec::new();
    match table.get("model") {
        Some(toml::Value::Array(items)) if !items.is_empty() => {
            for (index, item) in items.iter().enumerate() {
                let at = format!("model[{index}].");
                let Some(row) = item.as_table() else {
                    problems.push(format!("hand table: model[{index}] is not a table"));
                    continue;
                };
                let count = problems.len();
                for key in row.keys() {
                    if !HAND_MODEL_KEYS.contains(&key.as_str()) {
                        problems.push(format!("hand table: unknown key {at}{}", shown(key)));
                    }
                }
                let alias = hand_text(row, &at, "alias", problems);
                let id = hand_text(row, &at, "id", problems);
                let family = hand_text(row, &at, "family", problems);
                let default_effort = hand_text(row, &at, "default_effort", problems);
                let billing = hand_text(row, &at, "billing", problems);
                let rank = match row.get("rank") {
                    Some(toml::Value::Integer(n)) if *n >= 1 && *n <= i64::from(u32::MAX) => Some(*n as u32),
                    Some(_) => {
                        problems.push(format!("hand table: {at}rank is a whole number from 1"));
                        None
                    }
                    None => {
                        problems.push(format!("hand table: {at}rank is missing"));
                        None
                    }
                };
                let min_program = match row.get("min_program") {
                    None => None,
                    Some(toml::Value::String(s)) if is_version(s) => Some(s.clone()),
                    Some(_) => {
                        problems.push(format!("hand table: {at}min_program is a version like 2.1.300"));
                        None
                    }
                };
                let anthropic = hand_list(row, &at, "anthropic", true, problems);
                let epoch = hand_list(row, &at, "epoch", false, problems);
                for (key, value) in [("alias", &alias), ("id", &id), ("family", &family)] {
                    if let Some(v) = value {
                        if !is_plain_name(v) {
                            problems.push(format!("hand table: {at}{key} is not a plain name"));
                        }
                    }
                }
                if let Some(v) = &default_effort {
                    if !EFFORTS.contains(&v.as_str()) {
                        problems.push(format!("hand table: {at}default_effort is not an effort word"));
                    }
                }
                if let Some(v) = &billing {
                    if v != "plan" && v != "credits" {
                        problems.push(format!("hand table: {at}billing is plan or credits"));
                    }
                }
                if problems.len() != count {
                    continue;
                }
                if let (Some(alias), Some(id), Some(family), Some(rank), Some(default_effort), Some(billing)) =
                    (alias, id, family, rank, default_effort, billing)
                {
                    if models.iter().any(|m| m.alias == alias) {
                        problems.push(format!("hand table: alias {} appears twice", shown(&alias)));
                        continue;
                    }
                    models.push(HandModel { alias, id, family, rank, default_effort, billing, min_program, anthropic, epoch });
                }
            }
        }
        Some(toml::Value::Array(_)) | None => problems.push("hand table: no model rows".to_string()),
        Some(_) => problems.push("hand table: model is not a list of tables".to_string()),
    }
    // One Epoch name belongs to one model only.
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for model in &models {
        for name in &model.epoch {
            if let Some(other) = seen.insert(name.as_str(), model.alias.as_str()) {
                if other != model.alias {
                    problems.push(format!("hand table: the Epoch name {} belongs to {} and {}", shown(name), other, model.alias));
                }
            }
        }
    }
    if problems.len() == before { Some(Hand { epoch_scale, categories, models }) } else { None }
}

/// Whether the Anthropic id `api` is the one `wanted` names: the same id, or `wanted` and a dated suffix `-YYYYMMDD`.
fn id_matches(wanted: &str, api: &str) -> bool {
    match api.strip_prefix(wanted) {
        Some("") => true,
        Some(rest) => rest.strip_prefix('-').is_some_and(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())),
        None => false,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Anthropic's model list
// ---------------------------------------------------------------------------------------------------------------

/// What Anthropic says about one model that the hand table knows.
struct Listed {
    /// Index into `Hand::models`.
    hand: usize,
    efforts: Vec<&'static str>,
    status: &'static str,
    retire_on: Option<String>,
}

fn read_models(json: &str, hand: &Hand, problems: &mut Vec<String>) -> Vec<Listed> {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(_) => {
            problems.push("models.json: not valid JSON".to_string());
            return Vec::new();
        }
    };
    let Some(items) = value.get("data").and_then(|d| d.as_array()) else {
        problems.push("models.json: no \"data\" list".to_string());
        return Vec::new();
    };
    let mut out: Vec<Listed> = Vec::new();
    for item in items {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            problems.push("models.json: a model without an id".to_string());
            continue;
        };
        let matching: Vec<usize> = hand
            .models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.anthropic.iter().any(|w| id_matches(w, id)))
            .map(|(i, _)| i)
            .collect();
        let index = match matching.as_slice() {
            [] => {
                problems.push(format!("model {} is unknown to the hand table", shown(id)));
                continue;
            }
            [one] => *one,
            _ => {
                problems.push(format!("model {} matches more than one row of the hand table", shown(id)));
                continue;
            }
        };
        if out.iter().any(|l| l.hand == index) {
            problems.push(format!("model {} is a second answer for {}", shown(id), hand.models[index].alias));
            continue;
        }
        let count = problems.len();
        // The effort levels: the supported ones in the fixed order, none when the answer does not say.
        let effort = item.get("capabilities").and_then(|c| c.get("effort"));
        let mut efforts: Vec<&'static str> = Vec::new();
        if let Some(effort) = effort {
            for word in EFFORTS {
                if effort.get(word).and_then(|e| e.get("supported")).and_then(|s| s.as_bool()) == Some(true) {
                    efforts.push(word);
                }
            }
        }
        if efforts.is_empty() {
            problems.push(format!("model {} names no effort level (capabilities.effort)", shown(id)));
        }
        let status = match item.get("status").and_then(|s| s.as_str()) {
            Some(word) => match STATUSES.iter().find(|s| **s == word) {
                Some(status) => Some(*status),
                None => {
                    problems.push(format!("model {} has an unknown status", shown(id)));
                    None
                }
            },
            None => {
                problems.push(format!("model {} has no status", shown(id)));
                None
            }
        };
        let retire_on = match item.get("retire_on") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(day)) if is_date(day) => Some(day.clone()),
            Some(_) => {
                problems.push(format!("model {} has a retire_on that is no date", shown(id)));
                None
            }
        };
        let wanted = &hand.models[index].default_effort;
        if !efforts.is_empty() && !efforts.iter().any(|e| *e == wanted.as_str()) {
            problems.push(format!("model {} does not offer its default effort {}", shown(id), wanted));
        }
        if problems.len() != count {
            continue;
        }
        if let Some(status) = status {
            out.push(Listed { hand: index, efforts, status, retire_on });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Epoch priors
// ---------------------------------------------------------------------------------------------------------------

/// The priors per Epoch model name: category to the mean score in percent, rounded to a whole number.
fn read_priors(csv: &str, hand: &Hand, problems: &mut Vec<String>) -> BTreeMap<String, BTreeMap<&'static str, u8>> {
    let mut out: BTreeMap<String, BTreeMap<&'static str, u8>> = BTreeMap::new();
    let rows = match read_csv(csv) {
        Ok(rows) => rows,
        Err(why) => {
            problems.push(format!("epoch.csv: {why}"));
            return out;
        }
    };
    let Some((header, data)) = rows.split_first() else {
        problems.push("epoch.csv: no header line".to_string());
        return out;
    };
    let column = |name: &str| header.iter().position(|h| h.trim() == name);
    let (Some(model_at), Some(benchmark_at), Some(score_at)) = (column(CSV_MODEL), column(CSV_BENCHMARK), column(CSV_SCORE))
    else {
        problems.push(format!("epoch.csv: the columns {CSV_MODEL}, {CSV_BENCHMARK} and {CSV_SCORE} are needed"));
        return out;
    };
    let mut epoch_names: BTreeMap<&str, &str> = BTreeMap::new();
    for model in &hand.models {
        for name in &model.epoch {
            epoch_names.insert(name.as_str(), model.alias.as_str());
        }
    }
    // (model name, category) to the scores that count.
    let mut sums: BTreeMap<(String, &'static str), (f64, u32)> = BTreeMap::new();
    for (n, row) in data.iter().enumerate() {
        let line = n + 2;
        if row.len() != header.len() {
            problems.push(format!("epoch.csv: record {line} has {} fields, the header has {}", row.len(), header.len()));
            continue;
        }
        let model = row[model_at].trim();
        let benchmark = row[benchmark_at].trim();
        let Some(category) = hand.categories.get(benchmark) else { continue };
        if !epoch_names.contains_key(model) {
            continue;
        }
        let score = row[score_at].trim();
        if score.is_empty() {
            continue;
        }
        let Some(category) = CATEGORIES.iter().find(|c| **c == category.as_str()) else { continue };
        match score.parse::<f64>() {
            Ok(v) if v.is_finite() && v >= 0.0 && v * hand.epoch_scale <= 100.0 + 1e-9 => {
                let slot = sums.entry((model.to_string(), *category)).or_insert((0.0, 0));
                slot.0 += v * hand.epoch_scale;
                slot.1 += 1;
            }
            _ => problems.push(format!("epoch.csv: record {line} has a score that is no percentage")),
        }
    }
    for ((model, category), (sum, count)) in sums {
        let mean = (sum / f64::from(count)).round().clamp(0.0, 100.0) as u8;
        out.entry(model).or_default().insert(category, mean);
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// The text
// ---------------------------------------------------------------------------------------------------------------

/// A TOML basic string, written the way the core's `defs::toml_string` writes one (rewritten here: xtask does not
/// depend on skipper-core).
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 || u32::from(c) == 0x7f => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Builds the text of `model-catalog.toml` revision `rev`, written on `today` (`YYYY-MM-DD`), from the answer of
/// Anthropic (`models_json`), the Epoch table (`epoch_csv`) and the hand table (`hand`).
///
/// A model that Anthropic lists and the hand table does not know is named in the error list and the build is refused
/// (the hand table gets the new row first). A model of the hand table that Anthropic does not list is left out of the
/// candidate; the program keeps it from the layer below (the safety floor of the catalogue). No Epoch row for a model is
/// no error: the model has no priors.
///
/// # Errors
///
/// Every problem found, in the order of the sources: the hand table, the model list, the Epoch table, then `rev`,
/// `today` and the size of the result. Nothing partial is returned.
pub fn build(models_json: &str, epoch_csv: &str, hand: &str, rev: u32, today: &str) -> Result<String, Vec<String>> {
    let mut problems: Vec<String> = Vec::new();
    if rev == 0 {
        problems.push("the revision is 1 or more".to_string());
    }
    if !is_date(today) {
        problems.push("the day is YYYY-MM-DD".to_string());
    }
    let Some(hand) = read_hand(hand, &mut problems) else {
        return Err(problems);
    };
    let listed = read_models(models_json, &hand, &mut problems);
    let priors = read_priors(epoch_csv, &hand, &mut problems);
    if listed.is_empty() && problems.is_empty() {
        problems.push("models.json: no model of the hand table is listed".to_string());
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    let mut rows: Vec<(&HandModel, &Listed)> = listed.iter().map(|l| (&hand.models[l.hand], l)).collect();
    rows.sort_by(|a, b| a.0.rank.cmp(&b.0.rank).then_with(|| a.0.alias.cmp(&b.0.alias)));

    // Priors of the listed models, by alias, in the fixed category order.
    let mut any_prior = false;
    let mut model_priors: Vec<Vec<(&'static str, u8)>> = Vec::new();
    for (model, _) in &rows {
        let mut mine: Vec<(&'static str, u8)> = Vec::new();
        for category in CATEGORIES {
            // Every Epoch name of the model counts; the best of them is the model's own score for the category.
            let best = model.epoch.iter().filter_map(|name| priors.get(name).and_then(|p| p.get(category))).max();
            if let Some(score) = best {
                mine.push((category, *score));
                any_prior = true;
            }
        }
        model_priors.push(mine);
    }

    let mut text = String::new();
    text.push_str("# The model catalogue, built offline from the model list of Anthropic, the Epoch AI\n");
    text.push_str("# benchmark table and the hand table. Generated: do not edit; change the hand table and build again.\n\n");
    text.push_str(&format!("rev = {rev}\n"));
    text.push_str(&format!("generated = {}\n", toml_string(today)));
    let mut lines = vec![toml_string(ANTHROPIC_LINE)];
    if any_prior {
        lines.push(toml_string(EPOCH_LINE));
    }
    text.push_str(&format!("attribution = [{}]\n", lines.join(", ")));
    for ((model, listed), mine) in rows.iter().zip(&model_priors) {
        text.push_str("\n[[model]]\n");
        text.push_str(&format!("alias = {}\n", toml_string(&model.alias)));
        text.push_str(&format!("id = {}\n", toml_string(&model.id)));
        text.push_str(&format!("family = {}\n", toml_string(&model.family)));
        text.push_str(&format!("rank = {}\n", model.rank));
        let efforts: Vec<String> = listed.efforts.iter().map(|e| toml_string(e)).collect();
        text.push_str(&format!("efforts = [{}]\n", efforts.join(", ")));
        text.push_str(&format!("default_effort = {}\n", toml_string(&model.default_effort)));
        text.push_str(&format!("billing = {}\n", toml_string(&model.billing)));
        text.push_str(&format!("status = {}\n", toml_string(listed.status)));
        if let Some(day) = &listed.retire_on {
            text.push_str(&format!("retire_on = {}\n", toml_string(day)));
        }
        if let Some(version) = &model.min_program {
            text.push_str(&format!("min_program = {}\n", toml_string(version)));
        }
        for (category, score) in mine {
            text.push_str("\n[[model.prior]]\n");
            text.push_str(&format!("category = {}\n", toml_string(category)));
            text.push_str(&format!("score = {score}\n"));
            text.push_str(&format!("source = {}\n", toml_string(EPOCH_SOURCE)));
            text.push_str(&format!("date = {}\n", toml_string(today)));
        }
    }
    if text.len() > CATALOG_CAP {
        return Err(vec![format!("the catalogue is above {CATALOG_CAP} bytes")]);
    }
    Ok(text)
}
