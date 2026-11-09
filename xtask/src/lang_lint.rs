// SPDX-License-Identifier: MIT
//! `cargo xtask lang-lint` (plan Task 10; 6.4, S1G-13, MC-53, MC-94, MC-163, ACC-13): the checks on the language files.
//!
//! The files are `crates/skipper-app/lang/<code>/<area>.toml`, flat TOML (`skipper_core::lang` reads them). The rules:
//!
//! 0. every fragment is a flat table of texts, and no key stands in two fragments of one language;
//! 1. every key that code in `crates/*/src` passes to `t`/`tf`/`get`/`has` or gives to `Refusal::new`, and, in
//!    `skipper-core` and `skipper-app`, writes as a `key:`, `right_way:`, `message:` or `right:` constant, exists in
//!    the English fragments;
//! 2. no other language has a key English lacks, every English fragment has its German pair, and no fragment of
//!    another language lacks its English one;
//! 3. in `crates/skipper-app/src/{ui,vm}` and in the two files `crates/skipper-app/src/preview.rs` and
//!    `preview_main.rs` no string literal reaches `label(`, `button(`, `heading(`, `on_hover_text(`,
//!    `selectable_label(`, `checkbox(`, `hint_text(`, `menu_button(`, `collapsing(`, `hyperlink_to(`,
//!    `on_disabled_hover_text(`, `weak(`, `RichText::new(`, `Button::new(`, `Label::new(`, `Window::new(`,
//!    `CollapsingHeader::new(` or `ComboBox::from_label(` unless it is a key (the first argument of a lookup; the
//!    values given for placeholders are interface text); of `menu_button`, `collapsing` and `hyperlink_to` only the
//!    first argument is judged (the others are a closure or a URL; a call inside the closure is judged on its own);
//! 4. the text of `tier.t4.absent` holds no link, no label of a `button.*` key and no call to action (MC-53);
//! 5. the names of other products appear only under `import.*` keys (MC-94, 6.4);
//! 6. "official" is never within five words of "Skipper", and `first_run.safeguards.*`, when there is one, says
//!    "runs with Claude Code" and "not affiliated with Anthropic" (ACC-13, MC-163).
//!
//! Ruling (plan Task 10): a literal given to `get` or `has` counts as a key only when it looks like one (lower-case
//! segments joined by dots), because other code looks values up by name the same way (JSON and TOML tables);
//! `t` and `tf` are Skipper's own and take any literal. Test code is skipped in rules 1 and 3, because tests may use
//! made-up keys and plain texts: an item (a module, a function, a field, a variant, an arm, a parameter) that carries
//! a `cfg` that holds only in test builds (`test`, `all(test, ..)`), a file that starts with `#![cfg(..)]` of that kind,
//! and a file that another file declares as `#[cfg(..)] mod name;` of that kind. A `cfg` that can hold in a normal
//! build (`any(test, ..)`, `not(test)`) is judged; the reading is `rust_tokens`, shared with `provider-name-check`. A
//! `key:` inside a `DefError { ... }` literal holds a TOML key such as
//! "colour", not a language key. The constant fields (`key:`, `right_way:`, `message:`, `right:`) are read only in
//! the crates with interface text; protocol text for the model (`skipper-mcp`, `skipper-hook`, `skipper-standin`) stays
//! English in code and may use such field names. Rule 5 is a list of product names, not a guess: it grows with the
//! design. Cost if wrong: a lookup by name that looks like a key is reported (rename it), a product name that is not
//! on the list passes (add it), or a crate that gains interface text is added to `INTERFACE_CRATES`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::rust_tokens::{ident_at, is_ident, is_punct, is_test_file, skip_test_item, str_at, test_module_names, tokenize, Token};

/// The language files, relative to the workspace root (`cargo xtask` runs there).
const LANG_DIR: &str = "crates/skipper-app/lang";

/// The folder of the crates, relative to the workspace root.
const CRATES_DIR: &str = "crates";

/// The folders of interface code (rule 3), relative to the workspace root.
const UI_DIRS: [&str; 2] = ["crates/skipper-app/src/ui/", "crates/skipper-app/src/vm/"];

/// The two files of the preview that are interface code too (rule 3, M9): exact paths, not the rest of `src/`.
const UI_FILES: [&str; 2] = ["crates/skipper-app/src/preview.rs", "crates/skipper-app/src/preview_main.rs"];

/// The calls that put a text on the screen (rule 3), matched by their name.
const UI_CALLS: [&str; 12] = [
    "label",
    "button",
    "heading",
    "on_hover_text",
    "selectable_label",
    "checkbox",
    "hint_text",
    "menu_button",
    "collapsing",
    "hyperlink_to",
    "on_disabled_hover_text",
    "weak",
];

/// The calls of rule 3 that come with a path (`RichText::new`, with or without `egui::` in front), as (type, function).
const UI_PATH_CALLS: [(&str, &str); 6] = [
    ("RichText", "new"),
    ("Button", "new"),
    ("Label", "new"),
    ("Window", "new"),
    ("CollapsingHeader", "new"),
    ("ComboBox", "from_label"),
];

/// The calls of rule 3 whose text is the first argument only: the others are a URL (`hyperlink_to`) or a closure that
/// draws more (`menu_button`, `collapsing`). Calls inside that closure are found as calls of their own.
const FIRST_ARGUMENT_ONLY: [&str; 3] = ["menu_button", "collapsing", "hyperlink_to"];

/// The calls that look a key up. A literal inside their parentheses is a key or a value for a placeholder.
const LOOKUPS: [&str; 4] = ["t", "tf", "get", "has"];

/// The fields whose string constants are language keys (rule 1), and the crates in which they are read: the crates
/// with interface text. Protocol text for the model stays English in code, whatever its fields are called.
const KEY_FIELDS: [&str; 4] = ["key", "right_way", "message", "right"];
const INTERFACE_CRATES: [&str; 2] = ["skipper-core", "skipper-app"];

/// The languages whose folder must hold a pair for every English fragment.
const REQUIRED_PAIRS: [&str; 1] = ["de"];

/// The string shown for a missing Linux tier (rule 4).
const T4_ABSENT: &str = "tier.t4.absent";

/// Words that make a text a call to action, compared as whole words in lower case (rule 4).
const CALLS_TO_ACTION: [&str; 5] = ["install", "enable", "click", "open", "get"];

/// Names of other products that are distinct brand names (rule 5), compared as whole words in any letter case. The
/// names of the systems and the provider that Skipper must name (Windows, WSL, Microsoft, Claude Code, Anthropic) are
/// not on the lists.
const BRANDS: [&str; 11] =
    ["GitHub", "GitLab", "Jira", "Trello", "Asana", "ChatGPT", "OpenAI", "Gemini", "Copilot", "Confluence", "Obsidian"];

/// Names of other products that are also ordinary English words (rule 5), compared as whole words with their capital
/// letters as written, so that "a linear history" passes.
const PRODUCT_WORDS: [&str; 5] = ["Linear", "Notion", "Slack", "Cursor", "Codex"];

/// "official" next to "Skipper" counts when at most this many words lie between their positions (ACC-13).
const OFFICIAL_DISTANCE: usize = 5;

/// The two statements of ACC-13, in lower case. The second may also be the longer disclaimer of MC-163.
const RUNS_WITH: &str = "runs with claude code";
const NOT_AFFILIATED: [&str; 2] = ["not affiliated with anthropic", "not affiliated with or endorsed by anthropic"];

/// One language file: the folder it sits in (`en`), its file name (`base.toml`) and its text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fragment {
    pub language: String,
    pub name: String,
    pub text: String,
}

/// One source file: its path relative to the workspace root with `/` as separator, and its text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Source {
    pub path: String,
    pub text: String,
}

fn fragment_path(fragment: &Fragment) -> String {
    format!("{LANG_DIR}/{}/{}", fragment.language, fragment.name)
}

/// The keys and texts of a flat fragment text. Anything but a table of texts is an error.
fn entries(text: &str) -> Result<BTreeMap<String, String>, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for (key, value) in table {
        match value {
            toml::Value::String(shown) => {
                out.insert(key, shown);
            }
            _ => return Err(format!("`{key}` is not a text (write a key with dots in quotes: \"a.b\" = \"...\")")),
        }
    }
    Ok(out)
}

/// The words of a text: runs of letters and digits.
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect()
}

/// True when `needle` stands in `haystack` as a whole phrase: not inside a longer word.
fn contains_phrase(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(found) = haystack[from..].find(needle) {
        let start = from + found;
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = haystack[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// True when a text looks like a language key: two or more segments of lower-case letters, digits and underscores
/// joined by dots.
fn is_key_shaped(text: &str) -> bool {
    let mut segments = 0;
    for segment in text.split('.') {
        if segment.is_empty() || !segment.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
        segments += 1;
    }
    segments >= 2
}

/// The names of the modules that `src` declares at file level as `mod name;` and that exist in test builds only (every
/// declaration carries a test-only `cfg` and none a `#[path]`): their files are test code. The rule is
/// `rust_tokens::test_module_names`. Known limit, not covered by it: a shipping file below the folder of such a module
/// that another file reaches with `#[path]` is skipped with the folder (too late; the form is contrived and not in the
/// tree).
pub fn test_modules(src: &str) -> Vec<String> {
    test_module_names(&tokenize(src))
}

/// The folder that holds the files of the modules declared in the file `path`: the folder of a `mod.rs`, `lib.rs` or
/// `main.rs`, otherwise the file's own name without `.rs`.
fn module_folder(path: &str) -> String {
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    match file {
        "mod.rs" | "lib.rs" | "main.rs" => folder.to_owned(),
        _ => format!("{folder}/{}", file.strip_suffix(".rs").unwrap_or(file)),
    }
}

/// Rule 3: the string literals that reach an interface call in `src`, as (line, text). A literal is allowed as the key
/// of a lookup (the first argument of `t`, `tf`, `get`, `has`) and as the name in a placeholder pair given to one; the
/// values of the placeholders are interface text like any other. When it looks like a key, a literal is also allowed
/// inside any other call (a helper that looks the key up); a bare key still shows as the key, so it is reported.
/// A file that is test code as a whole (`#![cfg(test)]`) and the items that carry a test-only `cfg` are skipped.
pub fn ui_findings(src: &str) -> Vec<(usize, String)> {
    let tokens = tokenize(src);
    let mut out = Vec::new();
    if is_test_file(&tokens) {
        return out;
    }
    let mut i = 0;
    while i < tokens.len() {
        if let Some(end) = skip_test_item(&tokens, i) {
            i = end;
            continue;
        }
        let name = ident_at(&tokens, i).filter(|name| UI_CALLS.contains(name) && is_punct(&tokens, i + 1, '('));
        let path_call = UI_PATH_CALLS.iter().any(|(owner, function)| {
            is_ident(&tokens, i, owner)
                && is_punct(&tokens, i + 1, ':')
                && is_punct(&tokens, i + 2, ':')
                && is_ident(&tokens, i + 3, function)
                && is_punct(&tokens, i + 4, '(')
        });
        if let Some(name) = name {
            i = scan_call(&tokens, i + 1, FIRST_ARGUMENT_ONLY.contains(&name), &mut out);
        } else if path_call {
            i = scan_call(&tokens, i + 4, false, &mut out);
        } else {
            i += 1;
        }
    }
    out
}

/// One open parenthesis inside an interface call.
struct Paren {
    /// It opens a key lookup: `t`, `tf`, `get` or `has`.
    lookup: bool,
    /// It opens a tuple or a grouping, not a call.
    tuple: bool,
    /// The commas seen at this level: the index of the argument or element being read.
    at: usize,
}

/// Judges the arguments of the call whose `(` is at `open`, adds what rule 3 reports to `out` and returns the index
/// after the call's `)`. With `first_only` it judges the first argument alone and returns the index of the comma that
/// ends it: the scan goes on there, so a call in a later argument (in a closure) is judged as a call of its own.
fn scan_call(tokens: &[Token], open: usize, first_only: bool, out: &mut Vec<(usize, String)>) -> usize {
    let mut parens: Vec<Paren> = Vec::new();
    let mut i = open;
    while i < tokens.len() {
        if is_punct(tokens, i, '(') {
            let name = if i > 0 { ident_at(tokens, i - 1) } else { None };
            let lookup = name.is_some_and(|name| LOOKUPS.contains(&name));
            let macro_call = i > 0 && is_punct(tokens, i - 1, '!');
            parens.push(Paren { lookup, tuple: name.is_none() && !macro_call, at: 0 });
        } else if is_punct(tokens, i, ')') {
            parens.pop();
            if parens.is_empty() {
                return i + 1;
            }
        } else if is_punct(tokens, i, ',') {
            if first_only && parens.len() == 1 {
                return i;
            }
            if let Some(top) = parens.last_mut() {
                top.at += 1;
            }
        } else if let Token::Str(text, line) = &tokens[i] {
            let under_lookup = parens.iter().any(|paren| paren.lookup);
            let key_position = parens.last().is_some_and(|top| top.at == 0 && (top.lookup || (top.tuple && under_lookup)));
            let helper_key = parens.len() > 1 && is_key_shaped(text);
            if !key_position && !helper_key {
                out.push((*line, text.clone()));
            }
        }
        i += 1;
    }
    i
}

/// Rule 1: the language keys that code in `src` names, as (line, key): literals given to `.t(`, `.tf(`, `.get(` and
/// `.has(`, the arguments of `Refusal::new(`, and, when `constants` is set, string constants written after `key:`,
/// `right_way:`, `message:` and `right:`. Comments, the items that carry a test-only `cfg` and a file that is test code
/// as a whole are not code here.
pub fn used_keys(src: &str, constants: bool) -> Vec<(usize, String)> {
    let tokens = tokenize(src);
    let mut out = Vec::new();
    if is_test_file(&tokens) {
        return out;
    }
    // The name in front of each open `{`, to tell a `DefError { key: ... }` literal from any other.
    let mut owners: Vec<Option<&str>> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(end) = skip_test_item(&tokens, i) {
            i = end;
            continue;
        }
        if is_punct(&tokens, i, '{') {
            owners.push(if i > 0 { ident_at(&tokens, i - 1) } else { None });
        } else if is_punct(&tokens, i, '}') {
            owners.pop();
        } else if is_punct(&tokens, i, '.') {
            let call = ident_at(&tokens, i + 1).filter(|_| is_punct(&tokens, i + 2, '('));
            if let (Some(name), Some((text, line))) = (call, str_at(&tokens, i + 3)) {
                let own = matches!(name, "t" | "tf");
                let by_name = matches!(name, "get" | "has") && is_key_shaped(text);
                if own || by_name {
                    out.push((line, text.to_owned()));
                }
            }
        } else if let Some(name) = ident_at(&tokens, i) {
            if constants && KEY_FIELDS.contains(&name) && is_punct(&tokens, i + 1, ':') {
                if let Some((text, line)) = str_at(&tokens, i + 2) {
                    let toml_key = name == "key" && owners.last().copied().flatten() == Some("DefError");
                    if !toml_key {
                        out.push((line, text.to_owned()));
                    }
                }
            } else if name == "Refusal"
                && is_punct(&tokens, i + 1, ':')
                && is_punct(&tokens, i + 2, ':')
                && is_ident(&tokens, i + 3, "new")
                && is_punct(&tokens, i + 4, '(')
            {
                if let Some((text, line)) = str_at(&tokens, i + 5) {
                    out.push((line, text.to_owned()));
                    if is_punct(&tokens, i + 6, ',') {
                        if let Some((way, way_line)) = str_at(&tokens, i + 7) {
                            out.push((way_line, way.to_owned()));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Rule 1: the keys of `used` that the English text lacks, each once, in the order of first use.
pub fn key_findings(used: &[&str], english: &str) -> Vec<String> {
    let known = match entries(english) {
        Ok(known) => known,
        Err(why) => return vec![format!("the English text cannot be read: {why}")],
    };
    let mut out: Vec<String> = Vec::new();
    for key in used {
        if !known.contains_key(*key) && !out.iter().any(|seen| seen.as_str() == *key) {
            out.push((*key).to_owned());
        }
    }
    out
}

/// Rule 4: what is wrong with the text of `tier.t4.absent` in `text` (a language's merged fragments). The text may
/// not hold a link, the label of a `button.*` key, or a call to action; a text without that key has nothing to judge.
pub fn t4_findings(text: &str) -> Vec<String> {
    let all = match entries(text) {
        Ok(all) => all,
        Err(why) => return vec![why],
    };
    let Some(value) = all.get(T4_ABSENT) else {
        return Vec::new();
    };
    let lower = value.to_lowercase();
    let mut out = Vec::new();
    if lower.contains("http") || lower.contains("www") {
        out.push(format!("`{T4_ABSENT}` holds a link (MC-53, 6.4)"));
    }
    for (key, label) in all.iter().filter(|(key, _)| key.starts_with("button.")) {
        let label = label.trim().to_lowercase();
        if !label.is_empty() && contains_phrase(&lower, &label) {
            out.push(format!("`{T4_ABSENT}` holds the label of the button `{key}` (MC-53, 6.4)"));
        }
    }
    for word in words(&lower) {
        if CALLS_TO_ACTION.contains(&word) {
            out.push(format!("`{T4_ABSENT}` holds the call to action \"{word}\" (MC-53, 6.4)"));
        }
    }
    out
}

/// Rule 5: the keys of `text` outside `import.*` whose text names another product (see [`BRANDS`] and
/// [`PRODUCT_WORDS`]).
pub fn vendor_findings(text: &str) -> Vec<String> {
    let all = match entries(text) {
        Ok(all) => all,
        Err(why) => return vec![why],
    };
    let mut out = Vec::new();
    for (key, value) in &all {
        if key.starts_with("import.") {
            continue;
        }
        for word in value.split(|c: char| !c.is_alphanumeric()) {
            let brand = BRANDS.iter().any(|name| name.eq_ignore_ascii_case(word));
            if brand || PRODUCT_WORDS.contains(&word) {
                out.push(format!("`{key}` names the product {word}: only `import.*` keys may (MC-94, 6.4)"));
            }
        }
    }
    out
}

/// Rule 6: the keys of `text` that put "official" within [`OFFICIAL_DISTANCE`] words of "Skipper" (ACC-13, MC-163).
/// A word that starts with "skipper" is the name too ("Skippers", "Skipper-App"), and so is "offiziell..." for
/// "official".
pub fn acc13_findings(text: &str) -> Vec<String> {
    let all = match entries(text) {
        Ok(all) => all,
        Err(why) => return vec![why],
    };
    let mut out = Vec::new();
    for (key, value) in &all {
        let lower = value.to_lowercase();
        let mut skipper = Vec::new();
        let mut official = Vec::new();
        for (at, word) in words(&lower).iter().enumerate() {
            if word.starts_with("skipper") {
                skipper.push(at);
            } else if word.starts_with("official") || word.starts_with("offiziell") {
                official.push(at);
            }
        }
        if skipper.iter().any(|s| official.iter().any(|o| s.abs_diff(*o) <= OFFICIAL_DISTANCE)) {
            out.push(format!("`{key}` places \"official\" next to Skipper (ACC-13, MC-163)"));
        }
    }
    out
}

/// Rule 6: when `text` (the merged English fragments) has `first_run.safeguards.*` keys, together they say "runs with
/// Claude Code" and "not affiliated with Anthropic" (ACC-13, ACC-15).
pub fn safeguard_findings(text: &str) -> Vec<String> {
    let all = match entries(text) {
        Ok(all) => all,
        Err(why) => return vec![why],
    };
    let values: Vec<&str> =
        all.iter().filter(|(key, _)| key.starts_with("first_run.safeguards.")).map(|(_, value)| value.as_str()).collect();
    if values.is_empty() {
        return Vec::new();
    }
    let lower = values.join(" ").to_lowercase();
    let joined = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    if !joined.contains(RUNS_WITH) {
        out.push("`first_run.safeguards.*` never says \"runs with Claude Code\" (ACC-13)".to_owned());
    }
    if !NOT_AFFILIATED.iter().any(|phrase| joined.contains(*phrase)) {
        out.push("`first_run.safeguards.*` never says \"not affiliated with Anthropic\" (ACC-13)".to_owned());
    }
    out
}

/// The fragments of one language as one text, one after the other. Only meaningful when no key stands twice.
fn joined_text(fragments: &[Fragment], language: &str) -> String {
    let texts: Vec<&str> = fragments.iter().filter(|f| f.language == language).map(|f| f.text.as_str()).collect();
    texts.join("\n")
}

/// True for a Rust file below `crates/<crate>/src`.
fn is_crate_source(path: &str) -> bool {
    let mut parts = path.split('/');
    parts.next() == Some("crates") && parts.nth(1) == Some("src") && path.ends_with(".rs")
}

/// True when the crate of a source file reads the constant fields as language keys.
fn reads_constants(path: &str) -> bool {
    path.split('/').nth(1).is_some_and(|krate| INTERFACE_CRATES.contains(&krate))
}

/// All six rules over a language tree and the source files: report lines, none when everything holds. Rule 0 is
/// judged first and alone: with a broken or doubled fragment the merged texts the other rules read do not exist.
pub fn check_tree(fragments: &[Fragment], sources: &[Source]) -> Vec<String> {
    let mut report = Vec::new();

    // Rule 0: every fragment is a flat table of texts, and a key stands in one fragment of its language only.
    let mut languages: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    let mut owners: BTreeMap<(&str, String), String> = BTreeMap::new();
    for fragment in fragments {
        let path = fragment_path(fragment);
        match entries(&fragment.text) {
            Err(why) => report.push(format!("{path}: {why}")),
            Ok(found) => {
                let merged = languages.entry(fragment.language.as_str()).or_default();
                for (key, value) in found {
                    if let Some(first) = owners.get(&(fragment.language.as_str(), key.clone())) {
                        report.push(format!("{path}: the key `{key}` is also in {first}"));
                    } else {
                        owners.insert((fragment.language.as_str(), key.clone()), path.clone());
                        merged.insert(key, value);
                    }
                }
            }
        }
    }
    if !report.is_empty() {
        return report;
    }

    // Rule 2: no key beyond English, and a pair for every fragment.
    let Some(english) = languages.get("en") else {
        return vec![format!("{LANG_DIR}/en: there are no English language files")];
    };
    let english_names: BTreeSet<&str> = fragments.iter().filter(|f| f.language == "en").map(|f| f.name.as_str()).collect();
    let mut others: BTreeSet<&str> = REQUIRED_PAIRS.iter().copied().collect();
    others.extend(fragments.iter().map(|f| f.language.as_str()).filter(|language| *language != "en"));
    for language in &others {
        let names: BTreeSet<&str> = fragments.iter().filter(|f| f.language == *language).map(|f| f.name.as_str()).collect();
        if REQUIRED_PAIRS.contains(language) {
            for name in english_names.difference(&names) {
                report.push(format!("{LANG_DIR}/en/{name} has no pair {LANG_DIR}/{language}/{name}"));
            }
        }
        for name in names.difference(&english_names) {
            report.push(format!("{LANG_DIR}/{language}/{name} has no pair {LANG_DIR}/en/{name}"));
        }
        if let Some(merged) = languages.get(language) {
            for key in merged.keys().filter(|key| !english.contains_key(*key)) {
                report.push(format!("{LANG_DIR}/{language}: the key `{key}` has no English text"));
            }
        }
    }

    // The files that another file declares as `#[cfg(test)] mod name;` are test code.
    let mut test_bases: Vec<String> = Vec::new();
    for source in sources.iter().filter(|source| is_crate_source(&source.path)) {
        for name in test_modules(&source.text) {
            test_bases.push(format!("{}/{name}", module_folder(&source.path)));
        }
    }
    let is_test_path = |path: &str| {
        test_bases.iter().any(|base| path.strip_prefix(base.as_str()).is_some_and(|rest| rest == ".rs" || rest.starts_with('/')))
    };

    // Rule 1: every key that code names exists in English.
    let english_text = joined_text(fragments, "en");
    let mut used: Vec<(&str, usize, String)> = Vec::new();
    for source in sources.iter().filter(|source| is_crate_source(&source.path) && !is_test_path(&source.path)) {
        for (line, key) in used_keys(&source.text, reads_constants(&source.path)) {
            used.push((source.path.as_str(), line, key));
        }
    }
    let names: Vec<&str> = used.iter().map(|(_, _, key)| key.as_str()).collect();
    for key in key_findings(&names, &english_text) {
        for (path, line, used_key) in &used {
            if *used_key == key {
                report.push(format!("{path}:{line}: the key `{key}` is used in code but missing in the English files"));
            }
        }
    }

    // Rule 3: no literal reaches an interface call in the interface code.
    for source in sources {
        let interface_code = UI_DIRS.iter().any(|dir| source.path.starts_with(*dir)) || UI_FILES.contains(&source.path.as_str());
        if source.path.ends_with(".rs") && interface_code && !is_test_path(&source.path) {
            for (line, text) in ui_findings(&source.text) {
                report.push(format!(
                    "{}:{line}: the literal \"{text}\" reaches an interface call; use a language key (6.4, S1G-13)",
                    source.path
                ));
            }
        }
    }

    // Rule 4: the string for a missing Linux tier, in every language.
    for language in languages.keys() {
        for finding in t4_findings(&joined_text(fragments, language)) {
            let place = owners.get(&(*language, T4_ABSENT.to_owned())).cloned().unwrap_or_else(|| format!("{LANG_DIR}/{language}"));
            report.push(format!("{place}: {finding}"));
        }
    }

    // Rules 5 and 6: names of products and the wording of ACC-13, in every fragment.
    for fragment in fragments {
        let path = fragment_path(fragment);
        for finding in vendor_findings(&fragment.text).into_iter().chain(acc13_findings(&fragment.text)) {
            report.push(format!("{path}: {finding}"));
        }
    }
    for finding in safeguard_findings(&english_text) {
        report.push(format!("{LANG_DIR}/en: {finding}"));
    }
    report
}

/// The language fragments below `dir` (`<dir>/<code>/*.toml`), in folder and file name order.
fn read_fragments(dir: &Path) -> Result<Vec<Fragment>, String> {
    let mut folders: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect();
    folders.sort_by_key(|entry| entry.file_name());
    let mut out = Vec::new();
    for folder in folders {
        let language = folder.file_name().to_string_lossy().into_owned();
        let mut files: Vec<_> = fs::read_dir(folder.path())
            .map_err(|e| format!("cannot read {}: {e}", folder.path().display()))?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|x| x == "toml"))
            .collect();
        files.sort_by_key(|entry| entry.file_name());
        for file in files {
            let text = fs::read_to_string(file.path()).map_err(|e| format!("cannot read {}: {e}", file.path().display()))?;
            out.push(Fragment { language: language.clone(), name: file.file_name().to_string_lossy().into_owned(), text });
        }
    }
    Ok(out)
}

/// Every `.rs` file below `<dir>/<crate>/src`, with its path from the folder that holds `dir`, in name order.
fn read_sources(dir: &Path) -> Result<Vec<Source>, String> {
    fn walk(folder: &Path, relative: &str, out: &mut Vec<Source>) -> Result<(), String> {
        let mut items: Vec<_> = fs::read_dir(folder)
            .map_err(|e| format!("cannot read {}: {e}", folder.display()))?
            .filter_map(Result::ok)
            .collect();
        items.sort_by_key(|item| item.file_name());
        for item in items {
            let path = item.path();
            let shown = format!("{relative}/{}", item.file_name().to_string_lossy());
            if path.is_dir() {
                walk(&path, &shown, out)?;
            } else if path.extension().is_some_and(|x| x == "rs") {
                let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                out.push(Source { path: shown, text });
            }
        }
        Ok(())
    }
    let mut crates: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("src").is_dir())
        .collect();
    crates.sort_by_key(|entry| entry.file_name());
    let mut out = Vec::new();
    for entry in crates {
        let relative = format!("{CRATES_DIR}/{}/src", entry.file_name().to_string_lossy());
        walk(&entry.path().join("src"), &relative, &mut out)?;
    }
    Ok(out)
}

pub fn run_cli(args: &[String]) -> u8 {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask lang-lint");
        return 2;
    }
    if !Path::new(LANG_DIR).is_dir() {
        println!("lang-lint: no language files below {LANG_DIR} (run it from the workspace root)");
        return 1;
    }
    let fragments = match read_fragments(Path::new(LANG_DIR)) {
        Ok(fragments) => fragments,
        Err(why) => {
            println!("lang-lint: {why}");
            return 1;
        }
    };
    let sources = match read_sources(Path::new(CRATES_DIR)) {
        Ok(sources) => sources,
        Err(why) => {
            println!("lang-lint: {why}");
            return 1;
        }
    };
    let report = check_tree(&fragments, &sources);
    if report.is_empty() {
        println!("lang-lint: ok");
        0
    } else {
        for line in &report {
            println!("lang-lint: {line}");
        }
        println!("lang-lint: {} finding(s) (6.4, S1G-13)", report.len());
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The merged English text of a small language.
    const EN: &str = "\"view.empty.inbox\" = \"Nothing needs you right now.\"\n\"tier.t1\" = \"Files protected\"\n";

    /// A language tree of two files that agree with each other.
    const FIX_EN: &str = "\"view.title\" = \"Skipper\"\n\"view.hint\" = \"Use {n} roles\"\n";
    const FIX_DE: &str = "\"view.title\" = \"Skipper\"\n\"view.hint\" = \"Nutze {n} Rollen\"\n";

    fn found(src: &str) -> Vec<String> {
        used_keys(src, true).into_iter().map(|(_, key)| key).collect()
    }

    fn t4_value(value: &str) -> String {
        format!("\"tier.t4.absent\" = \"{value}\"")
    }

    fn fragment(language: &str, name: &str, text: &str) -> Fragment {
        Fragment { language: language.to_owned(), name: name.to_owned(), text: text.to_owned() }
    }

    fn source(path: &str, text: &str) -> Source {
        Source { path: path.to_owned(), text: text.to_owned() }
    }

    fn clean_fragments() -> Vec<Fragment> {
        vec![fragment("en", "base.toml", FIX_EN), fragment("de", "base.toml", FIX_DE)]
    }

    // The five tests of the plan.

    #[test]
    fn literal_in_a_label_call_is_reported() {
        assert_eq!(ui_findings("ui.label(\"Start\");").len(), 1);
        assert!(ui_findings("ui.label(lang.t(\"button.start\"));").is_empty());
    }

    #[test]
    fn key_used_in_code_but_missing_in_english_is_reported() {
        assert_eq!(key_findings(&["view.nope"], EN).len(), 1);
    }

    #[test]
    fn t4_absent_string_with_a_link_or_call_to_action_is_reported() {
        assert!(!t4_findings("\"tier.t4.absent\" = \"not available – install WSL at https://x\"").is_empty());
    }

    #[test]
    fn vendor_name_outside_import_keys_is_reported() {
        assert!(!vendor_findings("\"menu.x\" = \"Open Linear\"").is_empty());
        assert!(vendor_findings("\"import.linear\" = \"Import from Linear (CSV)\"").is_empty());
    }

    #[test]
    fn official_next_to_skipper_is_reported() {
        assert!(!acc13_findings("\"a\" = \"the official Skipper\"").is_empty());
    }

    // Rule 3: interface literals.

    #[test]
    fn every_listed_call_with_a_literal_is_reported() {
        for call in [
            "ui.label(\"Start\")",
            "ui.button(\"Start\")",
            "ui.heading(\"Start\")",
            "egui::RichText::new(\"Start\")",
            "response.on_hover_text(\"Start\")",
            "ui.selectable_label(true, \"Start\")",
            "ui.checkbox(&mut on, \"Start\")",
            "edit.hint_text(\"Start\")",
        ] {
            assert_eq!(ui_findings(&format!("{call};")).len(), 1, "{call}");
        }
    }

    #[test]
    fn keys_inside_lookups_and_helpers_are_not_literals() {
        for src in [
            "ui.label(lang.t(\"button.start\"));",
            "ui.button(lang.tf(\"x.y\", &[(\"n\", count)]));",
            "ui.heading(vm.text(\"view.title\"));",
            "ui.checkbox(&mut on, lang.t(\"setting.metrics\"));",
        ] {
            assert!(ui_findings(src).is_empty(), "{src}");
        }
    }

    #[test]
    fn a_bare_key_and_a_formatted_text_are_still_literals() {
        assert_eq!(ui_findings("ui.label(\"view.empty.inbox\");").len(), 1);
        assert_eq!(ui_findings("ui.label(format!(\"Hello {}\", name));").len(), 1);
        assert_eq!(ui_findings("ui.label(lang.t(\"a.b\").to_owned() + \"!\");").len(), 1);
    }

    #[test]
    fn a_literal_inside_one_call_is_reported_once() {
        assert_eq!(ui_findings("ui.label(egui::RichText::new(\"Start\").small());").len(), 1);
    }

    #[test]
    fn literals_outside_the_call_arguments_are_not_reported() {
        assert!(ui_findings("let s = \"Start\"; ui.label(text); let t = \"Stop\";").is_empty());
        assert!(ui_findings("fn label(&self, text: &str) {}").is_empty());
    }

    #[test]
    fn placeholder_values_are_interface_text_and_placeholder_names_are_not() {
        assert_eq!(ui_findings("ui.label(lang.tf(\"inbox.from\", &[(\"who\", \"the lead\")]));"), vec![(1, "the lead".to_owned())]);
        assert!(ui_findings("ui.label(lang.tf(\"inbox.from\", &[(\"who\", name)]));").is_empty());
        assert_eq!(ui_findings("ui.label(lang.tf(\"x.y\", &[(\"a\", first), (\"b\", \"text\")]));"), vec![(1, "text".to_owned())]);
    }

    #[test]
    fn only_the_first_argument_of_a_lookup_is_the_key() {
        assert_eq!(
            ui_findings("ui.label(lang.t(\"a.b\").to_owned() + lang.get(\"c.d\").unwrap_or(\"fallback\"));"),
            vec![(1, "fallback".to_owned())]
        );
    }

    #[test]
    fn comments_and_test_items_are_not_interface_code() {
        assert!(ui_findings("// ui.label(\"x\")\n/* ui.button(\"x\") */").is_empty());
        assert!(ui_findings("#[cfg(test)]\nmod tests { fn t() { ui.label(\"x\"); } }").is_empty());
        assert_eq!(ui_findings("#[cfg(test)]\nmod tests {}\nfn f() { ui.label(\"x\"); }").len(), 1);
    }

    #[test]
    fn the_line_and_the_literal_are_reported() {
        assert_eq!(ui_findings("\n\nui.label(\"Start\");"), vec![(3, "Start".to_owned())]);
    }

    // The ten calls added after the first version of the lint: the constructor paths with and without `egui::`, and
    // the methods.
    const NEW_CALLS: [&str; 10] = [
        "Button::new",
        "Label::new",
        "Window::new",
        "CollapsingHeader::new",
        "ComboBox::from_label",
        "ui.menu_button",
        "ui.collapsing",
        "ui.hyperlink_to",
        "response.on_disabled_hover_text",
        "text.weak",
    ];

    #[test]
    fn every_added_call_with_a_literal_is_reported() {
        for call in NEW_CALLS {
            for shown in [call.to_owned(), format!("egui::{call}")] {
                assert_eq!(ui_findings(&format!("{shown}(\"Start\");")), vec![(1, "Start".to_owned())], "{shown}");
            }
        }
    }

    #[test]
    fn every_added_call_with_a_key_through_a_lookup_reports_nothing() {
        for call in NEW_CALLS {
            for shown in [call.to_owned(), format!("egui::{call}")] {
                assert!(ui_findings(&format!("{shown}(lang.t(\"button.start\"));")).is_empty(), "{shown}");
                assert!(ui_findings(&format!("{shown}(lang.tf(\"x.y\", &[(\"n\", count)]));")).is_empty(), "{shown}");
            }
        }
    }

    #[test]
    fn a_name_that_only_ends_in_a_listed_constructor_is_not_that_call() {
        for src in ["MyButton::new(\"x\");", "Button::default(\"x\");", "Label::from_label(\"x\");", "fn weak(&self, text: &str) {}"] {
            assert!(ui_findings(src).is_empty(), "{src}");
        }
    }

    #[test]
    fn only_the_first_argument_of_hyperlink_to_is_interface_text() {
        assert_eq!(ui_findings("ui.hyperlink_to(\"Docs\", \"https://example.invalid/docs\");"), vec![(1, "Docs".to_owned())]);
        assert!(ui_findings("ui.hyperlink_to(lang.t(\"link.docs\"), \"https://example.invalid/docs\");").is_empty());
        assert!(ui_findings("ui.hyperlink_to(lang.t(\"link.docs\"), url.as_str());").is_empty());
    }

    #[test]
    fn the_closure_argument_of_menu_button_and_collapsing_is_not_an_argument_text() {
        for call in ["ui.menu_button", "ui.collapsing"] {
            let plain = format!("{call}(lang.t(\"menu.file\"), |ui| {{ let s = \"abc\"; run(s, \"x\"); }});");
            assert!(ui_findings(&plain).is_empty(), "{plain}");
            let titled = format!("{call}(\"File\", |ui| {{ let s = \"abc\"; run(s, \"x\"); }});");
            assert_eq!(ui_findings(&titled), vec![(1, "File".to_owned())], "{titled}");
        }
    }

    #[test]
    fn calls_inside_a_closure_are_scanned_as_their_own_calls() {
        for call in ["ui.menu_button", "ui.collapsing"] {
            let literal = format!("{call}(lang.t(\"menu.file\"), |ui| {{ ui.label(\"Inner\"); }});");
            assert_eq!(ui_findings(&literal), vec![(1, "Inner".to_owned())], "{literal}");
            let keyed = format!("{call}(lang.t(\"menu.file\"), |ui| {{ ui.label(lang.t(\"menu.open\")); }});");
            assert!(ui_findings(&keyed).is_empty(), "{keyed}");
        }
    }

    #[test]
    fn a_window_built_with_a_key_and_shown_with_a_closure_reports_nothing() {
        let src = "egui::Window::new(lang.t(\"form.title\")).show(ctx, |ui| { ui.label(lang.t(\"form.hint\")); });";
        assert!(ui_findings(src).is_empty());
        let literal = "egui::Window::new(\"Form\").show(ctx, |ui| { ui.label(lang.t(\"form.hint\")); });";
        assert_eq!(ui_findings(literal), vec![(1, "Form".to_owned())]);
    }

    // `#[cfg(test)]` on a field, a variant, an arm, a parameter or a generic parameter hides that one thing and no more
    // (the fix of `provider_names`, ported): the shipping code after it is still judged, for an interface literal ("x")
    // and for a key English lacks ("x.y").
    #[test]
    fn a_cfg_test_element_hides_only_itself() {
        let shipping = "ui.label(\"x\"); l.t(\"x.y\");";
        for (src, line) in [
            (format!("struct S {{ #[cfg(test)] f: u8 }}\nfn g() {{ {shipping} }}"), 2),
            (format!("struct S {{ a: u8, #[cfg(test)] f: u8, }}\nfn g() {{ {shipping} }}"), 2),
            (format!("enum E {{ A, #[cfg(test)] B }}\nfn g() {{ {shipping} }}"), 2),
            (format!("enum E {{ A, #[cfg(test)] B(u8), C }}\nfn g() {{ {shipping} }}"), 2),
            (format!("fn f(v: u8) {{ match v {{ 0 => 1, #[cfg(test)] 1 => 2 }} {shipping} }}"), 1),
            (format!("fn f(a: u8, #[cfg(test)] b: u8) {{ {shipping} }}"), 1),
            (format!("fn f<T, #[cfg(test)] U>(x: T) {{ {shipping} }}"), 1),
        ] {
            assert_eq!(ui_findings(&src), vec![(line, "x".to_owned())], "{src}");
            assert_eq!(found(&src), ["x.y"], "{src}");
        }
    }

    // Test code that is not an item-level `#[cfg(test)]`.
    #[test]
    fn an_inner_cfg_test_makes_the_whole_file_test_code() {
        for src in [
            "#![cfg(test)]\nfn t() { ui.label(\"x\"); l.t(\"x.y\"); }",
            "//! doc line\n#![allow(dead_code)]\n#![cfg(test)]\nfn t() { ui.label(\"x\"); l.t(\"x.y\"); }",
            "#![cfg(all(test, windows))]\nfn t() { ui.label(\"x\"); l.t(\"x.y\"); }",
        ] {
            assert!(ui_findings(src).is_empty(), "{src}");
            assert!(found(src).is_empty(), "{src}");
        }
        let judged = "#![cfg(not(test))]\nfn t() { ui.label(\"x\"); l.t(\"x.y\"); }";
        assert_eq!(ui_findings(judged).len(), 1);
        assert_eq!(found(judged), ["x.y"]);
    }

    #[test]
    fn a_cfg_that_holds_only_in_test_builds_is_test_code() {
        let body = "mod m { fn f() { ui.label(\"x\"); l.t(\"x.y\"); } }";
        // `all(test, ..)` needs `test` to hold, whatever else it says. `any(test, ..)` can hold in a normal build.
        for attribute in ["#[cfg(all(test, windows))]", "#[cfg(all(test, not(windows)))]"] {
            let src = format!("{attribute}\n{body}");
            assert!(ui_findings(&src).is_empty(), "{attribute}");
            assert!(found(&src).is_empty(), "{attribute}");
        }
        for attribute in [
            "#[cfg(any(test, feature = \"fixtures\"))]",
            "#[cfg(not(test))]",
            "#[cfg(windows)]",
            "#[cfg(feature = \"test\")]",
            "#[cfg_attr(test, derive(Debug))]",
        ] {
            let src = format!("{attribute}\n{body}");
            assert_eq!(ui_findings(&src).len(), 1, "{attribute}");
            assert_eq!(found(&src), ["x.y"], "{attribute}");
        }
    }

    // Rule 1: keys used in code.

    #[test]
    fn keys_are_found_in_lookups_and_in_constants() {
        let src = "a.t(\"a.t\"); a.tf(\"a.tf\", &[]); a.get(\"a.get\"); a.has(\"a.has\"); \
                   Foo { key: \"a.key\", right_way: \"a.way\", message: \"a.msg\", right: \"a.right\" }; \
                   Refusal::new(\"a.r1\", \"a.r2\");";
        assert_eq!(found(src), ["a.t", "a.tf", "a.get", "a.has", "a.key", "a.way", "a.msg", "a.right", "a.r1", "a.r2"]);
    }

    #[test]
    fn without_constants_only_lookups_and_refusals_are_keys() {
        let src = "Foo { key: \"a.k\", message: \"a.m\" }; l.t(\"a.t\"); Refusal::new(\"a.r1\", \"a.r2\");";
        let keys = |constants| used_keys(src, constants).into_iter().map(|(_, key)| key).collect::<Vec<_>>();
        assert_eq!(keys(true), ["a.k", "a.m", "a.t", "a.r1", "a.r2"]);
        assert_eq!(keys(false), ["a.t", "a.r1", "a.r2"]);
    }

    #[test]
    fn lookups_of_json_values_are_not_keys() {
        assert!(found("v.get(\"agent_id\"); v.get(\"Some.Thing\"); m.has(\"x\"); v.get(\"a.B\");").is_empty());
    }

    #[test]
    fn t_and_tf_take_any_literal_and_a_dynamic_key_is_not_judged() {
        assert_eq!(found("l.t(\"plain\"); l.tf(\"Plain\", &[]);"), ["plain", "Plain"]);
        assert!(found("l.t(&key); l.t(format!(\"tier.{n}\")); l.tf(key, &[]);").is_empty());
    }

    #[test]
    fn a_key_field_of_a_def_error_holds_a_toml_key_and_is_not_a_language_key() {
        assert_eq!(found("DefError { key: \"colour\", message: \"def.bad\".into() }"), ["def.bad"]);
        assert_eq!(found("DefError { key: \"x\", inner: Foo { key: \"a.k\" }, right: \"a.r\" }"), ["a.k", "a.r"]);
        assert_eq!(found("DefError { key: \"x\" } Foo { key: \"a.k\" }"), ["a.k"]);
    }

    #[test]
    fn a_field_type_or_a_path_is_not_a_constant() {
        assert!(found("struct S { key: &'static str, message: String } fn f(key: &str) {}").is_empty());
        assert!(found("let k = key::NAME; let m = message::build(\"a.b\");").is_empty());
    }

    #[test]
    fn comments_and_test_items_hold_no_keys() {
        assert!(found("// l.t(\"a.b\")\n#[cfg(test)]\nmod tests { fn f() { l.t(\"x.y\"); } }").is_empty());
        assert_eq!(found("#[cfg(test)]\nmod tests {}\nfn f() { l.t(\"x.y\"); }"), ["x.y"]);
    }

    #[test]
    fn the_line_of_a_key_is_reported() {
        assert_eq!(used_keys("\n\nl.t(\"a.b\");", true), vec![(3, "a.b".to_owned())]);
    }

    #[test]
    fn a_key_english_has_is_not_reported_and_a_missing_one_once() {
        assert!(key_findings(&["view.empty.inbox", "tier.t1"], EN).is_empty());
        assert_eq!(key_findings(&["view.nope", "view.nope", "tier.t1", "tier.nope"], EN), ["view.nope", "tier.nope"]);
    }

    // Rule 4: the string for a missing Linux tier.

    #[test]
    fn the_plain_t4_string_passes() {
        assert!(t4_findings(&t4_value("not available on this PC")).is_empty());
    }

    #[test]
    fn a_link_in_the_t4_string_is_reported() {
        assert!(!t4_findings(&t4_value("see www.example.org")).is_empty());
        assert!(!t4_findings(&t4_value("see HTTP://example.org")).is_empty());
    }

    #[test]
    fn each_call_to_action_word_in_the_t4_string_is_reported() {
        for word in ["install", "Enable", "CLICK", "open", "Get"] {
            assert!(!t4_findings(&t4_value(&format!("not available, {word} it"))).is_empty(), "{word}");
        }
    }

    #[test]
    fn a_word_that_only_contains_a_call_to_action_is_not_one() {
        assert!(t4_findings(&t4_value("not available on this target budget")).is_empty());
    }

    #[test]
    fn a_button_label_in_the_t4_string_is_reported_but_not_inside_a_longer_word() {
        let with = |value: &str| format!("\"button.add_wsl\" = \"Add WSL\"\n\"button.go\" = \"Go\"\n{}", t4_value(value));
        assert!(!t4_findings(&with("not available, add WSL first")).is_empty());
        assert!(!t4_findings(&with("not available, go away")).is_empty());
        assert!(t4_findings(&with("not available, gone")).is_empty());
    }

    #[test]
    fn a_text_without_the_t4_key_passes_and_an_unreadable_text_is_reported() {
        assert!(t4_findings("\"a\" = \"Install\"").is_empty());
        assert!(!t4_findings("tier = \"x\" [oops").is_empty());
    }

    // Rule 5: names of other products.

    #[test]
    fn every_listed_product_name_is_reported_outside_import_keys() {
        for name in BRANDS.iter().chain(PRODUCT_WORDS.iter()) {
            assert!(!vendor_findings(&format!("\"menu.x\" = \"Use {name} here\"")).is_empty(), "{name}");
            assert!(vendor_findings(&format!("\"import.x.y\" = \"Import from {name}\"")).is_empty(), "{name}");
        }
        for name in BRANDS {
            for shown in [name.to_lowercase(), name.to_uppercase()] {
                assert!(!vendor_findings(&format!("\"menu.x\" = \"Use {shown} here\"")).is_empty(), "{shown}");
            }
        }
    }

    #[test]
    fn brand_names_are_reported_in_any_letter_case_but_plain_words_only_as_written() {
        assert_eq!(vendor_findings("\"a\" = \"Github, GITHUB and github.com/x\"").len(), 3);
        assert!(!vendor_findings("\"a\" = \"see gitlab\"").is_empty());
        assert!(!vendor_findings("\"a\" = \"CHATGPT\"").is_empty());
        assert!(!vendor_findings("\"a\" = \"Linear and Notion\"").is_empty());
        assert!(vendor_findings("\"a\" = \"linear, notion and slack\"").is_empty());
    }

    #[test]
    fn system_and_provider_names_are_not_reported_and_neither_are_longer_words() {
        assert!(vendor_findings("\"a\" = \"Windows, WSL, Microsoft, Claude Code and Anthropic\"").is_empty());
        assert!(vendor_findings("\"a\" = \"a linear history, Linearly, notional\"").is_empty());
        assert!(!vendor_findings("\"a\" = \"GitHub's settings\"").is_empty());
    }

    // Rule 6: the name check of ACC-13.

    #[test]
    fn official_within_five_words_of_skipper_is_reported_in_both_directions() {
        for text in [
            "official one two three four Skipper",
            "Skipper, the official one",
            "OFFICIAL SKIPPER",
            "das offizielle Skipper",
            "die offizielle Skipper-Version",
        ] {
            assert_eq!(acc13_findings(&format!("\"a\" = \"{text}\"")).len(), 1, "{text}");
        }
    }

    #[test]
    fn official_far_from_skipper_or_with_the_disclaimer_is_not_reported() {
        for text in [
            "official one two three four five Skipper",
            "Skipper is the only tool made by a person who is not official",
            "Skipper is not affiliated with or endorsed by Anthropic",
            "unofficial Skipper",
        ] {
            assert!(acc13_findings(&format!("\"a\" = \"{text}\"")).is_empty(), "{text}");
        }
    }

    #[test]
    fn the_safeguards_hold_both_phrases_when_there_are_safeguards() {
        let both = "\"first_run.safeguards.one\" = \"Skipper RUNS WITH Claude Code.\"\n\"first_run.safeguards.two\" = \"It is not affiliated\\n  with Anthropic.\"";
        assert!(safeguard_findings(both).is_empty());
        let longer = "\"first_run.safeguards.one\" = \"Skipper runs with Claude Code and is not affiliated with or endorsed by Anthropic.\"";
        assert!(safeguard_findings(longer).is_empty());
        let one = "\"first_run.safeguards.one\" = \"Skipper runs with Claude Code.\"";
        assert_eq!(safeguard_findings(one).len(), 1);
        assert_eq!(safeguard_findings("\"first_run.safeguards.one\" = \"Plain.\"").len(), 2);
    }

    #[test]
    fn without_safeguards_there_is_nothing_to_check_and_other_first_run_keys_do_not_count() {
        assert!(safeguard_findings("\"a\" = \"b\"").is_empty());
        let other = "\"first_run.intro\" = \"Skipper runs with Claude Code, not affiliated with Anthropic.\"\n\"first_run.safeguards.one\" = \"Plain.\"";
        assert_eq!(safeguard_findings(other).len(), 2);
    }

    #[test]
    fn the_german_genitive_and_compounds_of_skipper_count_as_skipper() {
        for text in ["die offizielle Version Skippers", "Skippers offizielle Seite", "offizielle Skipper-App"] {
            assert_eq!(acc13_findings(&format!("\"a\" = \"{text}\"")).len(), 1, "{text}");
        }
    }

    // The whole tree.

    #[test]
    fn a_clean_tree_has_no_findings() {
        let sources = [
            source("crates/skipper-core/src/x.rs", "fn f(l: &Lang) { l.t(\"view.title\"); }"),
            source("crates/skipper-app/src/ui/a.rs", "fn g() { ui.label(lang.t(\"view.hint\")); }"),
        ];
        assert_eq!(check_tree(&clean_fragments(), &sources), Vec::<String>::new());
    }

    #[test]
    fn a_key_in_two_fragments_of_one_language_is_reported_with_both_files() {
        let mut fragments = clean_fragments();
        fragments.push(fragment("en", "extra.toml", "\"view.title\" = \"Again\"\n"));
        fragments.push(fragment("de", "extra.toml", ""));
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("view.title") && report[0].contains("en/base.toml") && report[0].contains("en/extra.toml"), "{report:?}");
    }

    #[test]
    fn a_fragment_that_does_not_parse_or_is_not_flat_is_reported() {
        for bad in ["x = ", "[tier]\nt1 = \"x\"", "a = 1", "a.b = \"x\""] {
            let mut fragments = clean_fragments();
            fragments.push(fragment("en", "bad.toml", bad));
            fragments.push(fragment("de", "bad.toml", ""));
            let report = check_tree(&fragments, &[]);
            assert_eq!(report.len(), 1, "{bad}: {report:?}");
            assert!(report[0].contains("en/bad.toml"), "{report:?}");
        }
    }

    #[test]
    fn a_german_key_the_english_files_lack_is_reported() {
        let mut fragments = clean_fragments();
        fragments[1].text.push_str("\"view.extra\" = \"Extra\"\n");
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("view.extra") && report[0].contains("/de"), "{report:?}");
    }

    #[test]
    fn every_english_file_needs_its_german_pair_and_the_other_way_round() {
        let mut fragments = clean_fragments();
        fragments.push(fragment("en", "more.toml", "\"more.a\" = \"A\"\n"));
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("more.toml") && report[0].contains("de/more.toml"), "{report:?}");

        let mut fragments = clean_fragments();
        fragments.push(fragment("de", "lone.toml", ""));
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("de/lone.toml") && report[0].contains("en/lone.toml"), "{report:?}");

        // No German folder at all: the one English file has no pair.
        let report = check_tree(&[fragment("en", "base.toml", FIX_EN)], &[]);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("de/base.toml"), "{report:?}");
    }

    #[test]
    fn another_language_may_be_incomplete_but_may_not_have_extra_keys() {
        let mut fragments = clean_fragments();
        fragments.push(fragment("fr", "base.toml", "\"view.title\" = \"Skipper\"\n"));
        assert_eq!(check_tree(&fragments, &[]), Vec::<String>::new());
        fragments[2].text.push_str("\"view.nope\" = \"x\"\n");
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("view.nope") && report[0].contains("/fr"), "{report:?}");
    }

    #[test]
    fn a_key_used_in_code_but_missing_in_english_is_reported_with_file_and_line() {
        let sources = [source("crates/skipper-core/src/x.rs", "fn f() {\n    l.t(\"view.nope\");\n}")];
        let report = check_tree(&clean_fragments(), &sources);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].starts_with("crates/skipper-core/src/x.rs:2:") && report[0].contains("view.nope"), "{report:?}");
    }

    #[test]
    fn only_source_files_of_the_crates_are_judged_for_keys() {
        let code = "fn f() { l.t(\"view.nope\"); }";
        for path in ["crates/skipper-core/tests/x.rs", "xtask/src/x.rs", "crates/skipper-core/src/data.toml"] {
            assert!(check_tree(&clean_fragments(), &[source(path, code)]).is_empty(), "{path}");
        }
        assert_eq!(check_tree(&clean_fragments(), &[source("crates/skipper-app/src/model/m.rs", code)]).len(), 1);
    }

    #[test]
    fn interface_literals_are_judged_in_ui_and_vm_only() {
        let code = "fn f() { ui.label(\"Start\"); }";
        for path in ["crates/skipper-app/src/ui/a.rs", "crates/skipper-app/src/vm/deep/b.rs"] {
            let report = check_tree(&clean_fragments(), &[source(path, code)]);
            assert_eq!(report.len(), 1, "{path}: {report:?}");
            assert!(report[0].starts_with(path), "{report:?}");
        }
        for path in ["crates/skipper-app/src/app.rs", "crates/skipper-core/src/ui/a.rs", "crates/skipper-app/src/panels.rs"] {
            assert!(check_tree(&clean_fragments(), &[source(path, code)]).is_empty(), "{path}");
        }
    }

    #[test]
    fn the_two_preview_files_are_interface_code_and_no_other_file_of_src_is() {
        let code = "fn f() { ui.label(\"Start\"); }";
        for path in ["crates/skipper-app/src/preview.rs", "crates/skipper-app/src/preview_main.rs"] {
            let report = check_tree(&clean_fragments(), &[source(path, code)]);
            assert_eq!(report.len(), 1, "{path}: {report:?}");
            assert!(report[0].starts_with(path), "{report:?}");
            let keyed = "fn f() { ui.label(lang.t(\"view.hint\")); }";
            assert!(check_tree(&clean_fragments(), &[source(path, keyed)]).is_empty(), "{path}");
        }
        for path in [
            "crates/skipper-app/src/preview_extra.rs",
            "crates/skipper-app/src/preview/a.rs",
            "crates/skipper-app/src/win/preview.rs",
            "crates/skipper-core/src/preview.rs",
        ] {
            assert!(check_tree(&clean_fragments(), &[source(path, code)]).is_empty(), "{path}");
        }
    }

    #[test]
    fn a_test_only_preview_file_is_not_interface_code() {
        let sources = [
            source("crates/skipper-app/src/preview.rs", "#[cfg(test)]\nmod tests;"),
            source("crates/skipper-app/src/preview/tests.rs", "fn t() { ui.label(\"x\"); }"),
            source("crates/skipper-app/src/preview_main.rs", "#[cfg(test)]\nmod t { fn f() { ui.label(\"x\"); } }"),
        ];
        assert!(check_tree(&clean_fragments(), &sources).is_empty());
    }

    #[test]
    fn the_constant_fields_are_judged_only_in_the_crates_with_interface_text() {
        let code = "fn f() -> McpError { McpError { code: 1, message: \"invalid parameters\".into() } }";
        for path in ["crates/skipper-mcp/src/x.rs", "crates/skipper-hook/src/x.rs", "crates/skipper-standin/src/x.rs"] {
            assert!(check_tree(&clean_fragments(), &[source(path, code)]).is_empty(), "{path}");
        }
        for path in ["crates/skipper-app/src/x.rs", "crates/skipper-core/src/x.rs"] {
            let report = check_tree(&clean_fragments(), &[source(path, code)]);
            assert_eq!(report.len(), 1, "{path}: {report:?}");
            assert!(report[0].contains("invalid parameters"), "{report:?}");
        }
        // A lookup is Skipper's own call wherever it stands.
        let lookup = "fn f() { l.t(\"view.nope\"); }";
        assert_eq!(check_tree(&clean_fragments(), &[source("crates/skipper-mcp/src/x.rs", lookup)]).len(), 1);
    }

    #[test]
    fn a_file_declared_as_a_cfg_test_module_is_test_code() {
        let test_text = "fn t() { l.t(\"x.y\"); }";
        for (declaring, declared, declaration) in [
            ("crates/skipper-core/src/a.rs", "crates/skipper-core/src/a/tests.rs", "#[cfg(test)]\nmod tests;"),
            ("crates/skipper-core/src/a/mod.rs", "crates/skipper-core/src/a/tests.rs", "#[cfg(test)]\nmod tests;"),
            ("crates/skipper-core/src/lib.rs", "crates/skipper-core/src/tests.rs", "#[cfg(test)]\npub(crate) mod tests;"),
            ("crates/skipper-core/src/a.rs", "crates/skipper-core/src/a/tests/deep.rs", "#[cfg(test)] #[allow(dead_code)] mod tests;"),
            ("crates/skipper-core/src/a.rs", "crates/skipper-core/src/a/tests/mod.rs", "#[cfg(all(test, windows))]\nmod tests;"),
        ] {
            let with = [source(declaring, declaration), source(declared, test_text)];
            assert!(check_tree(&clean_fragments(), &with).is_empty(), "{declaring} -> {declared}");
            // Without the declaration (or with a plain `mod`) the same file is product code.
            let without = [source(declaring, "mod tests;"), source(declared, test_text)];
            assert_eq!(check_tree(&clean_fragments(), &without).len(), 1, "{declaring} -> {declared}");
        }
        // An inline test module declares no file, and a file elsewhere is not touched by another file's declaration.
        let inline = [source("crates/skipper-core/src/a.rs", "#[cfg(test)]\nmod tests {}"), source("crates/skipper-core/src/a/tests.rs", test_text)];
        assert_eq!(check_tree(&clean_fragments(), &inline).len(), 1);
        let other = [source("crates/skipper-core/src/a.rs", "#[cfg(test)]\nmod tests;"), source("crates/skipper-core/src/b/tests.rs", test_text)];
        assert_eq!(check_tree(&clean_fragments(), &other).len(), 1);
    }

    #[test]
    fn a_ui_file_declared_as_a_cfg_test_module_is_not_interface_code() {
        let sources = [
            source("crates/skipper-app/src/ui/panel.rs", "#[cfg(test)]\nmod tests;"),
            source("crates/skipper-app/src/ui/panel/tests.rs", "fn t() { ui.label(\"x\"); }"),
        ];
        assert!(check_tree(&clean_fragments(), &sources).is_empty());
    }

    #[test]
    fn the_text_rules_name_the_fragment_that_holds_the_text() {
        let mut fragments = clean_fragments();
        fragments[0].text.push_str("\"menu.x\" = \"Open Linear\"\n\"about.y\" = \"the official Skipper\"\n\"tier.t4.absent\" = \"install it\"\n");
        fragments[1].text.push_str("\"menu.x\" = \"Linear öffnen\"\n\"about.y\" = \"das offizielle Skipper\"\n\"tier.t4.absent\" = \"nicht verfügbar\"\n");
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 5, "{report:?}");
        assert_eq!(report.iter().filter(|line| line.contains("en/base.toml")).count(), 3, "{report:?}");
        assert_eq!(report.iter().filter(|line| line.contains("de/base.toml")).count(), 2, "{report:?}");
    }

    #[test]
    fn the_safeguards_are_judged_in_english_only() {
        let mut fragments = clean_fragments();
        fragments[0].text.push_str("\"first_run.safeguards.one\" = \"Plain.\"\n");
        let report = check_tree(&fragments, &[]);
        assert_eq!(report.len(), 2, "{report:?}");
    }

    #[test]
    fn the_base_fragments_of_this_checkout_pass_every_rule() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace root").join(LANG_DIR);
        let read = |language: &str| fs::read_to_string(dir.join(language).join("base.toml")).expect("the base fragment can be read");
        let fragments = [fragment("en", "base.toml", &read("en")), fragment("de", "base.toml", &read("de"))];
        assert_eq!(check_tree(&fragments, &[]), Vec::<String>::new());
    }
}
