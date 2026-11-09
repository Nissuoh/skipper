// SPDX-License-Identifier: MIT
//! `cargo xtask skill-lint` (Stage 1 Task 46c; MC-215, MC-163, MC-105, G9; KITS 2.4): the checks on Skipper's own skills,
//! `crates/skipper-core/data/skills/<name>/{SKILL.md, skill.toml, reference.md}`.
//!
//! Every rule gives a finding with the skill's name, a rule id and one sentence:
//!
//! - sizes: description at most 240 characters; body at most 120 lines and 5,000 bytes; `reference.md` at most 8,000
//!   bytes; `SKILL.md` plus `reference.md` at most 13,000 bytes;
//! - the frontmatter: only `name`, `description`, `license`; `paths` and `disable-model-invocation` only with a
//!   `reason` line in `skill.toml`; `allowed-tools`, `hooks`, `model` and `effort` never; any other key is unknown;
//! - the name: kebab-case, at most 28 characters, the same in the folder, the frontmatter and `skill.toml`, unique, no
//!   "claude" or "anthropic", not in `RESERVED`, no vendor word;
//! - the text: no URL, no command for python, py, pip, node, npm or npx, the SPDX line, no leak-list word, no grabbing
//!   word in the description;
//! - the folder: no executable or script file, by extension and by PE or ELF magic;
//! - `shipped = true` only with `docs/skills/results/<name>.toml` that names the same version and hash and says
//!   `verdict = "pass"`; a provenance entry (`docs/skills/provenance.toml`, `[[source]]` with `skill = "<name>"`) for
//!   every skill whose version is above 0;
//! - every folder of `WAVE1` exists and no other folder does (the list is the one of `skipper_core::skills::WAVE1`;
//!   this crate does not depend on the core, and the core's own test lists the same 17 names);
//! - every skill that `duties.toml` names has a folder.
//!
//! The leak-list words: the real term list never leaves the main PC (the template lint says the same), so this lint
//! checks the words of the shape that never belong in a skill (a profile path, a mail address of the commit identity,
//! words that mark a text as not for the public); the per-commit `leak-scan` reads every committed file with the real
//! list.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The skills of wave 1: the base list and the role table of the catalogue. The same names as
/// `skipper_core::skills::WAVE1`.
pub const WAVE1: [&str; 17] = [
    "done-with-evidence",
    "clear-handoff",
    "root-cause-debugging",
    "change-hygiene",
    "rust-idioms",
    "task-breakdown",
    "requirements-interview",
    "design-direction",
    "visual-check",
    "failure-paths",
    "schema-migration",
    "ci-failure-triage",
    "test-first",
    "security-diff-check",
    "independent-review",
    "source-research",
    "doc-writing",
];

/// The names Claude Code ships as bundled skills, as its skill listing showed them on 2026-10-26 (KITS 1.1). Which of two
/// same-named skills wins is not known, so a Skipper skill never takes one of these names.
pub const RESERVED: &[&str] = &[
    "code-review",
    "security-review",
    "simplify",
    "init",
    "loop",
    "schedule",
    "run",
    "review",
    "batch",
    "debug",
    "claude-api",
    "update-config",
    "fewer-permission-prompts",
    "keybindings-help",
    "plugin-authoring",
    "workflow-authoring",
];

/// Product and vendor names that no skill name carries (as a whole word of the kebab-case name).
const VENDOR_WORDS: &[&str] = &[
    "openai", "chatgpt", "gpt", "gemini", "copilot", "cursor", "codex", "windsurf", "cline", "aider", "superpowers", "github",
    "gitlab", "jetbrains", "vscode",
];

/// The tool words of the command rule; a skill never tells a role to run any of them (D7, G9).
const COMMAND_WORDS: &[&str] = &["python", "python3", "py", "pip", "pip3", "node", "npm", "npx"];

/// File name extensions of executables and scripts (MC-105, G9).
const SCRIPT_EXTENSIONS: &[&str] = &[
    "py", "pyw", "js", "mjs", "cjs", "ts", "sh", "bash", "zsh", "ps1", "psm1", "bat", "cmd", "vbs", "wsf", "rb", "pl", "php", "exe",
    "dll", "so", "dylib", "com", "msi", "jar", "scr", "lnk",
];

/// Words that make a description grab for every task (HUNTER:90), in lower case.
const GRABBING_WORDS: &[&str] = &[
    "always use",
    "use always",
    "for every",
    "every task",
    "every request",
    "all tasks",
    "critical",
    "mandatory",
    "must be used",
    "you must use",
    "never skip",
];

/// Words that mark a text as not for the public, in lower case. Profile paths and the commit identity's mail domain are
/// added by `leak_hit`, built from pieces so that this file does not hold them.
const LEAK_WORDS: &[&str] = &["confidential", "internal use only", "do not distribute", "proprietary"];

/// The frontmatter keys of a skill.
const ALLOWED_KEYS: &[&str] = &["name", "description", "license"];
/// Keys that need a reason line in `skill.toml`.
const REASON_KEYS: &[&str] = &["paths", "disable-model-invocation"];
/// Keys that are never allowed.
const REFUSED_KEYS: &[&str] = &["allowed-tools", "hooks", "model", "effort"];

const MAX_DESCRIPTION: usize = 240;
const MAX_BODY_LINES: usize = 120;
const MAX_BODY_BYTES: usize = 5_000;
const MAX_REFERENCE: usize = 8_000;
const MAX_WHOLE: usize = 13_000;
const MAX_NAME: usize = 28;

/// Where the command looks, relative to the workspace root.
const SKILLS_DIR: &str = "crates/skipper-core/data/skills";
const RESULTS_DIR: &str = "docs/skills/results";
const PROVENANCE: &str = "docs/skills/provenance.toml";
const DUTIES: &str = "crates/skipper-core/data/duties.toml";

/// One thing wrong with a skill.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SkillFinding {
    pub skill: String,
    pub rule: &'static str,
    pub detail: String,
}

fn found(out: &mut Vec<SkillFinding>, skill: &str, rule: &'static str, detail: impl Into<String>) {
    out.push(SkillFinding { skill: skill.to_string(), rule, detail: detail.into() });
}

/// The lower-case hex SHA-256 of the bytes.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let raw: &[u8] = digest.as_ref();
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

/// `SKILL.md` split into its frontmatter keys and the body; `None` when there is no frontmatter block.
fn split_frontmatter(text: &str) -> Option<(Vec<(String, String)>, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut offset = first.len();
    let mut pairs = Vec::new();
    for line in lines {
        offset += line.len();
        if line.trim_end() == "---" {
            return Some((pairs, &text[offset..]));
        }
        if let Some((key, value)) = line.split_once(':') {
            pairs.push((key.trim().to_string(), value.trim().to_string()));
        } else if !line.trim().is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
            pairs.push((line.trim().to_string(), String::new()));
        }
    }
    None
}

/// The first URL-like word of a text, if any.
fn url_hit(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    ["http://", "https://", "ftp://", "www."].into_iter().find(|p| lower.contains(*p))
}

/// The tool word of the command rule that a text uses as a command: as the first word of a line (after list marks, a
/// prompt sign or a back tick) or as the first word inside a back-tick span.
fn command_hit(text: &str) -> Option<&'static str> {
    let first_word = |s: &str| -> Option<&'static str> {
        let word: String = s
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '$' | '>' | '-' | '*' | '`'))
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        COMMAND_WORDS.iter().copied().find(|w| *w == word)
    };
    for line in text.lines() {
        if let Some(w) = first_word(line) {
            return Some(w);
        }
        for (n, span) in line.split('`').enumerate() {
            if n % 2 == 1 {
                if let Some(w) = first_word(span) {
                    return Some(w);
                }
            }
        }
    }
    None
}

/// A leak-list word of a text, if any.
fn leak_hit(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if let Some(w) = LEAK_WORDS.iter().find(|w| lower.contains(**w)) {
        return Some((*w).to_string());
    }
    // A profile path of Windows or Linux, and the mail domain of the commit identity: built from pieces.
    let shapes = [[":", "\\users\\"].concat(), ["/", "home/"].concat(), ["noreply", ".github.com"].concat()];
    shapes.into_iter().find(|s| lower.contains(s.as_str()))
}

/// A grabbing word of a description, if any.
fn grabbing_hit(description: &str) -> Option<&'static str> {
    let lower = description.to_lowercase();
    GRABBING_WORDS.iter().copied().find(|w| lower.contains(*w))
}

/// Whether bytes start like a Windows or Unix executable.
fn has_exe_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MZ") || bytes.starts_with(&[0x7f, b'E', b'L', b'F'])
}

/// Every regular file below `dir`, with `/` paths relative to `dir`.
fn files_below(dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                walk(root, &path, out);
            } else if kind.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
                    out.push((rel, path));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// The rules about a skill's name, for the name written in its folder.
fn name_findings(out: &mut Vec<SkillFinding>, name: &str) {
    let kebab = !name.is_empty()
        && name.split('-').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    if !kebab {
        found(out, name, "name-format", "the name is not kebab-case (lower-case letters, digits and single hyphens)");
    }
    if name.chars().count() > MAX_NAME {
        found(out, name, "name-length", format!("the name is longer than {MAX_NAME} characters"));
    }
    if name.contains("claude") || name.contains("anthropic") {
        found(out, name, "name-forbidden-word", "the name holds \"claude\" or \"anthropic\"");
    }
    if RESERVED.contains(&name) {
        found(out, name, "name-reserved", "the name is one of Claude Code's bundled skills");
    }
    if let Some(word) = name.split('-').find(|part| VENDOR_WORDS.contains(part)) {
        found(out, name, "name-vendor", format!("the name holds the vendor word `{word}`"));
    }
}

/// The skills a provenance file lists, by `skill = "<name>"` of its `[[source]]` tables.
fn provenance_names(provenance: &Path) -> Option<BTreeSet<String>> {
    let text = fs::read_to_string(provenance).ok()?;
    let table: toml::Table = text.parse().ok()?;
    let sources = table.get("source")?.as_array()?;
    Some(sources.iter().filter_map(|s| s.get("skill")?.as_str().map(str::to_string)).collect())
}

/// The checks on one skill folder. `folder` is the folder's name.
fn lint_folder(out: &mut Vec<SkillFinding>, folder: &str, dir: &Path, results_dir: &Path, provenance: &BTreeSet<String>, have_provenance: bool, names: &mut BTreeSet<String>) {
    name_findings(out, folder);
    let files = files_below(dir);
    for (rel, path) in &files {
        let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        if SCRIPT_EXTENSIONS.contains(&ext.as_str()) {
            found(out, folder, "script-file", format!("`{rel}` is an executable or script file (extension)"));
        } else if fs::read(path).map(|b| has_exe_magic(&b)).unwrap_or(false) {
            found(out, folder, "script-file", format!("`{rel}` is an executable file (PE or ELF header)"));
        }
    }

    let read = |name: &str| -> Option<Vec<u8>> { fs::read(dir.join(name)).ok() };
    let Some(skill_bytes) = read("SKILL.md") else {
        found(out, folder, "skill-md", "SKILL.md is missing");
        return;
    };
    let Ok(skill_md) = String::from_utf8(skill_bytes.clone()) else {
        found(out, folder, "skill-md", "SKILL.md is not UTF-8 text");
        return;
    };
    let reference_bytes = read("reference.md");
    let reference = reference_bytes.as_ref().and_then(|b| String::from_utf8(b.clone()).ok());
    let meta_text = read("skill.toml").and_then(|b| String::from_utf8(b).ok());
    let meta: Option<toml::Table> = match &meta_text {
        None => {
            found(out, folder, "skill-toml", "skill.toml is missing");
            None
        }
        Some(text) => match text.parse() {
            Ok(t) => Some(t),
            Err(e) => {
                found(out, folder, "skill-toml", format!("skill.toml does not parse: {e}"));
                None
            }
        },
    };

    // Sizes.
    let (pairs, body) = match split_frontmatter(&skill_md) {
        Some((pairs, body)) => (pairs, body),
        None => {
            found(out, folder, "frontmatter-missing", "SKILL.md has no frontmatter block");
            (Vec::new(), skill_md.as_str())
        }
    };
    let description = pairs.iter().find(|(k, _)| k == "description").map(|(_, v)| v.as_str()).unwrap_or("");
    if !pairs.is_empty() && description.is_empty() {
        found(out, folder, "frontmatter-missing", "the frontmatter has no description");
    }
    let desc_chars = description.chars().count();
    if desc_chars > MAX_DESCRIPTION {
        found(out, folder, "size-description", format!("the description has {desc_chars} characters, over {MAX_DESCRIPTION}"));
    }
    let body_lines = body.lines().count();
    if body_lines > MAX_BODY_LINES {
        found(out, folder, "size-body-lines", format!("the body has {body_lines} lines, over {MAX_BODY_LINES}"));
    }
    if body.len() > MAX_BODY_BYTES {
        found(out, folder, "size-body-bytes", format!("the body has {} bytes, over {MAX_BODY_BYTES}", body.len()));
    }
    let reference_len = reference_bytes.as_ref().map_or(0, Vec::len);
    if reference_len > MAX_REFERENCE {
        found(out, folder, "size-reference", format!("reference.md has {reference_len} bytes, over {MAX_REFERENCE}"));
    }
    let whole = skill_bytes.len() + reference_len;
    if whole > MAX_WHOLE {
        found(out, folder, "size-whole", format!("SKILL.md and reference.md hold {whole} bytes, over {MAX_WHOLE}"));
    }

    // Frontmatter keys.
    let reason = meta.as_ref().and_then(|m| m.get("reason")).and_then(|r| r.as_str()).unwrap_or("");
    for (key, _) in &pairs {
        let key = key.as_str();
        if ALLOWED_KEYS.contains(&key) {
            continue;
        }
        if REFUSED_KEYS.contains(&key) {
            found(out, folder, "key-refused", format!("the frontmatter key `{key}` is refused"));
        } else if REASON_KEYS.contains(&key) {
            if reason.trim().is_empty() {
                found(out, folder, "key-needs-reason", format!("the frontmatter key `{key}` needs a `reason` line in skill.toml"));
            }
        } else {
            found(out, folder, "key-unknown", format!("the frontmatter key `{key}` is unknown"));
        }
    }

    // Names: frontmatter, skill.toml and folder agree; unique.
    let fm_name = pairs.iter().find(|(k, _)| k == "name").map(|(_, v)| v.as_str()).unwrap_or("");
    if !pairs.is_empty() && fm_name != folder {
        found(out, folder, "name-mismatch", format!("the frontmatter name `{fm_name}` is not the folder name"));
    }
    if let Some(toml_name) = meta.as_ref().and_then(|m| m.get("name")).and_then(|n| n.as_str()) {
        if toml_name != folder {
            found(out, folder, "name-mismatch", format!("the skill.toml name `{toml_name}` is not the folder name"));
        }
    }
    if !names.insert(fm_name.to_string()) && !fm_name.is_empty() {
        found(out, folder, "name-duplicate", format!("the name `{fm_name}` is used by another skill"));
    }

    // The text.
    let mut texts: Vec<(&str, &str)> = vec![("SKILL.md", skill_md.as_str())];
    if let Some(r) = &reference {
        texts.push(("reference.md", r.as_str()));
    }
    if let Some(m) = &meta_text {
        texts.push(("skill.toml", m.as_str()));
    }
    for (file, text) in &texts {
        if url_hit(text).is_some() {
            found(out, folder, "url", format!("{file} holds a URL"));
        }
        if let Some(w) = leak_hit(text) {
            found(out, folder, "leak-word", format!("{file} holds a leak-list word (`{w}`)"));
        }
    }
    for (file, text) in texts.iter().filter(|(file, _)| *file != "skill.toml") {
        if let Some(w) = command_hit(text) {
            found(out, folder, "command-word", format!("{file} tells to run `{w}`"));
        }
    }
    if !skill_md.contains("SPDX-License-Identifier: MIT") {
        found(out, folder, "spdx", "SKILL.md has no `SPDX-License-Identifier: MIT` line");
    }
    if let Some(w) = grabbing_hit(description) {
        found(out, folder, "grabbing-word", format!("the description grabs with `{w}`"));
    }

    // skill.toml fields, the verdict and the provenance.
    let Some(meta) = meta else { return };
    let kind = meta.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let text_of = |key: &str| meta.get(key).and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "base-all" | "base-code" => {}
        "duty" if !text_of("duty").is_empty() => {}
        "stack" if !text_of("stack").is_empty() => {}
        "duty" | "stack" => found(out, folder, "skill-toml", format!("kind `{kind}` needs its `{kind}` field")),
        _ => found(out, folder, "skill-toml", format!("kind `{kind}` is not base-all, base-code, duty or stack")),
    }
    let version = meta.get("version").and_then(|v| v.as_integer());
    if version.is_none() {
        found(out, folder, "skill-toml", "`version` is missing or not a number");
    }
    if version.unwrap_or(0) > 0 && (!have_provenance || !provenance.contains(folder)) {
        found(out, folder, "provenance-missing", "version is above 0 and docs/skills/provenance.toml has no entry for the skill");
    }
    if meta.get("shipped").and_then(|s| s.as_bool()).unwrap_or(false) {
        let hash = {
            let mut bytes = skill_bytes.clone();
            if let Some(r) = &reference_bytes {
                bytes.extend_from_slice(r);
            }
            sha256_hex(&bytes)
        };
        let verdict: Option<toml::Table> = fs::read_to_string(results_dir.join(format!("{folder}.toml"))).ok().and_then(|t| t.parse().ok());
        match verdict {
            None => found(out, folder, "shipped-no-verdict", "shipped = true, but there is no readable docs/skills/results/<name>.toml"),
            Some(v) => {
                if v.get("verdict").and_then(|x| x.as_str()) != Some("pass") {
                    found(out, folder, "shipped-no-verdict", "shipped = true, but the verdict is not \"pass\"");
                }
                let same_version = v.get("version").and_then(|x| x.as_integer()) == version;
                let same_hash = v.get("sha256").and_then(|x| x.as_str()) == Some(hash.as_str()) && text_of("sha256") == hash;
                if !same_version || !same_hash {
                    found(out, folder, "shipped-verdict-mismatch", "shipped = true, but the verdict names another version or text hash");
                }
            }
        }
    }
}

/// Lints every skill folder below `skills_dir`; `results_dir` holds the verdicts, `provenance` the idea sources. An empty
/// result means the tree is clean.
pub fn lint_skills(skills_dir: &Path, results_dir: &Path, provenance: &Path) -> Vec<SkillFinding> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(skills_dir) else {
        found(&mut out, "-", "skills-dir", format!("cannot read {}", skills_dir.display()));
        return out;
    };
    let mut folders: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    folders.sort();
    let sources = provenance_names(provenance);
    let have_provenance = sources.is_some();
    let sources = sources.unwrap_or_default();
    let mut names = BTreeSet::new();
    for (name, dir) in &folders {
        lint_folder(&mut out, name, dir, results_dir, &sources, have_provenance, &mut names);
    }
    for wanted in WAVE1 {
        if !folders.iter().any(|(n, _)| n == wanted) {
            found(&mut out, wanted, "wave1-missing", "the folder of a wave-1 skill is missing");
        }
    }
    for (name, _) in &folders {
        if !WAVE1.contains(&name.as_str()) {
            found(&mut out, name, "wave1-extra", "the folder is not a wave-1 skill (WAVE1)");
        }
    }
    out
}

/// The skills `duties.toml` names (the `[base]` lists and the `skills` of each duty) that have no folder in `skills_dir`.
pub fn lint_duty_skills(duties_toml: &str, skills_dir: &Path) -> Vec<SkillFinding> {
    let mut out = Vec::new();
    let Ok(table) = duties_toml.parse::<toml::Table>() else {
        found(&mut out, "-", "duties", "duties.toml does not parse");
        return out;
    };
    let mut named: BTreeSet<String> = BTreeSet::new();
    if let Some(base) = table.get("base").and_then(|b| b.as_table()) {
        for list in base.values().filter_map(|v| v.as_array()) {
            named.extend(list.iter().filter_map(|n| n.as_str().map(str::to_string)));
        }
    }
    if let Some(duties) = table.get("duty").and_then(|d| d.as_array()) {
        for duty in duties {
            let skills = duty.get("skills").and_then(|s| s.as_array());
            named.extend(skills.into_iter().flatten().filter_map(|s| s.get("name")?.as_str().map(str::to_string)));
        }
    }
    for name in named {
        if !skills_dir.join(&name).join("SKILL.md").is_file() {
            found(&mut out, &name, "duty-skill-missing", "duties.toml names the skill, but it has no folder");
        }
    }
    out
}

/// The workspace root: the parent of this crate's folder, so that `cargo test` (current folder `xtask/`) and `cargo xtask`
/// find the same tree.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
}

pub fn run_cli(args: &[String]) -> u8 {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask skill-lint");
        return 2;
    }
    let root = workspace_root();
    let skills_dir = root.join(SKILLS_DIR);
    let mut findings = lint_skills(&skills_dir, &root.join(RESULTS_DIR), &root.join(PROVENANCE));
    match fs::read_to_string(root.join(DUTIES)) {
        Ok(text) => findings.extend(lint_duty_skills(&text, &skills_dir)),
        Err(_) => found(&mut findings, "-", "duties", format!("cannot read {DUTIES}")),
    }
    if findings.is_empty() {
        let folders = fs::read_dir(&skills_dir).map(|d| d.flatten().filter(|e| e.path().is_dir()).count()).unwrap_or(0);
        let shipped = fs::read_dir(&skills_dir)
            .map(|d| {
                d.flatten()
                    .filter(|e| {
                        fs::read_to_string(e.path().join("skill.toml"))
                            .ok()
                            .and_then(|t| t.parse::<toml::Table>().ok())
                            .and_then(|t| t.get("shipped").and_then(|s| s.as_bool()))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        println!("skill-lint: ok ({folders} skills, {shipped} shipped)");
        0
    } else {
        for f in &findings {
            println!("skill-lint: {} {} {}", f.skill, f.rule, f.detail);
        }
        println!("skill-lint: {} problem(s) (MC-215)", findings.len());
        1
    }
}
