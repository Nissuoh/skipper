// SPDX-License-Identifier: MIT
//! The network half of the catalogue job. It exists only with
//! the feature `catalog-ci`, so `xtask.exe` (the push gate's scanner) links no network crate: the dependency
//! test of `deps-check` refuses an edge from the default build of `xtask` to `skipper-net`.
//!
//! The subcommands of the binary `xtask-catalog` (`src/bin/xtask-catalog.rs`, which starts with
//! [`crate::guard::require_capped_or_ci`]):
//!
//! - `fetch --out <dir>`: the model list of Anthropic (`GET /v1/models`) and the Epoch table, into `<dir>`. The key is the
//!   CI variable [`KEY_VARIABLE`]; it is read in [`fetch_cli`] only, handed on as a borrowed text, sent as one request
//!   header and never printed, logged or written. Without a key (none is set yet) nothing is asked of Anthropic and
//!   no `models.json` is written: the job then builds from Epoch and the hand table and publishes nothing.
//! - `build --in <dir> [--hand <file>] [--out <file>]`: [`crate::catalog::build`] over the fetched files. Without
//!   `models.json` it only checks that the Epoch table and the hand table read (a list derived from the hand table
//!   stands in for Anthropic's) and writes no candidate: nothing unverified can ever be published.
//! - `publish --file <file>`: uploads `model-catalog.toml`, a marker asset for its content and a new `SHA256SUMS.txt` to
//!   the release `catalog` with `GITHUB_TOKEN`, only when the content changed and only with the repository variable
//!   `CATALOG_PUBLISH = "yes"` (the owner's standing yes) passed as the environment variable of that name.
//! - `cc-fetch --out <dir>`: the docs half: the three docs pages, the SchemaStore file and the newest Claude Code
//!   version, into `<dir>`. The catalogue diff that reads these files follows with the code.
//!
//! Every request goes through the `Transport` trait of `skipper-net` (the fake in the tests; WinHTTP in the job), with
//! the hosts of its own list, caps of 1 MiB (4 MiB for a docs page) and no redirect on a request that holds a secret.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use skipper_net::http::{
    fetch, get_following, system_transport, BodySink, Flow, Header, Limits, Method, NetError, Request, ResponseHead, Transport, Url,
};

/// The CI variable that holds an API key the maintainer provides. It is read in [`fetch_cli`] and nowhere else.
pub const KEY_VARIABLE: &str = "SKIPPER_CATALOG_ANTHROPIC_KEY";
/// The repository variable that records the owner's standing yes to publishing; the workflow passes it on.
pub const PUBLISH_VARIABLE: &str = "CATALOG_PUBLISH";

pub const ANTHROPIC_HOST: &str = "api.anthropic.com";
/// The model list, up to 1000 entries in one answer (an answer that says there is more is refused).
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const EPOCH_HOST: &str = "epoch.ai";
/// GUESS: the research report names no address. The probe day checks it against
/// the real hub; a wrong address is a refused fetch (404), never a wrong catalogue.
pub const EPOCH_URL: &str = "https://epoch.ai/data/benchmark_data/benchmarks_runs.csv";

pub const GITHUB_API_HOST: &str = "api.github.com";
pub const GITHUB_UPLOADS_HOST: &str = "uploads.github.com";
/// The two hosts the publish may reach.
pub const PUBLISH_HOSTS: [&str; 2] = [GITHUB_API_HOST, GITHUB_UPLOADS_HOST];

/// The docs pages and the schema the diff reads (GUESS: the `.md` form of the docs pages; a redirect to any other host
/// is refused, and a wrong address is a refused fetch).
pub const DOCS_HOST: &str = "code.claude.com";
pub const SCHEMA_HOST: &str = "json.schemastore.org";
pub const DOCS_HOSTS: [&str; 2] = [DOCS_HOST, SCHEMA_HOST];
pub const DOCS_FILES: [(&str, &str); 4] = [
    ("settings-reference.md", "https://code.claude.com/docs/en/settings.md"),
    ("env-vars.md", "https://code.claude.com/docs/en/env-vars.md"),
    ("cli-reference.md", "https://code.claude.com/docs/en/cli-reference.md"),
    ("schema.json", "https://json.schemastore.org/claude-code-settings.json"),
];
/// GUESS: Claude Code publishes its versions as GitHub releases; the tag is `v<major>.<minor>.<patch>`.
pub const CLAUDE_RELEASE_URL: &str = "https://api.github.com/repos/anthropics/claude-code/releases/latest";
pub const VERSION_FILE: &str = "claude-version.txt";

/// The release that holds the data files and the names of its assets.
pub const RELEASE_TAG: &str = "catalog";
pub const MODEL_FILE: &str = "model-catalog.toml";
pub const SUMS_FILE: &str = "SHA256SUMS.txt";
/// The name of the marker asset starts with this; the rest is the first 32 hex digits of the content fingerprint.
pub const MARKER_PREFIX: &str = "content-sha256-";

pub const FETCHED_MODELS: &str = "models.json";
pub const FETCHED_EPOCH: &str = "epoch.csv";

/// The cap of one answer of the model fetch and the largest file of the build: 1 MiB.
pub const MAX_ANSWER: u64 = 1024 * 1024;
/// The cap of one docs page: 4 MiB.
pub const MAX_DOC: u64 = 4 * 1024 * 1024;
const TOTAL_TIME: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 3;
const USER_AGENT: &str = "skipper-catalog-ci";

const DEFAULT_HAND: &str = "xtask/data/model-hand.toml";
const DEFAULT_CANDIDATE: &str = "target/catalog/model-catalog.toml";

// ---- small helpers -------------------------------------------------------------------------------------------------

/// A refusal as text. It never holds a secret: the texts of `NetError` are static and the callers add no header value.
#[derive(Debug)]
struct Refused(String);

impl From<NetError> for Refused {
    fn from(error: NetError) -> Refused {
        Refused(error.to_string())
    }
}

fn net(error: NetError) -> String {
    error.to_string()
}

/// Collects the body and remembers the status.
#[derive(Default)]
struct Collect {
    status: u16,
    bytes: Vec<u8>,
}

impl BodySink for Collect {
    fn head(&mut self, head: &ResponseHead) -> Flow {
        self.status = head.status;
        Flow::More
    }

    fn chunk(&mut self, data: &[u8]) -> Flow {
        self.bytes.extend_from_slice(data);
        Flow::More
    }
}

fn limits(max: u64) -> Limits {
    Limits::new(max, TOTAL_TIME)
}

fn host_of(url: &str, hosts: &[&str]) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(net)?;
    if hosts.contains(&parsed.host()) {
        Ok(parsed)
    } else {
        Err(format!("the host {} is not one of the allowed hosts", parsed.host()))
    }
}

/// One GET without a secret: redirects are followed by hand, and only to the hosts in `hosts`.
fn get_public(t: &dyn Transport, url: &str, accept: &str, hosts: &[&str], max: u64) -> Result<Vec<u8>, String> {
    let start = host_of(url, hosts)?;
    let mut sink = Collect::default();
    let allow = |target: &str| -> Result<Url, Refused> {
        let parsed = Url::parse(target)?;
        if hosts.contains(&parsed.host()) {
            Ok(parsed)
        } else {
            Err(Refused(format!("a redirect to {} is refused", parsed.host())))
        }
    };
    let got = get_following::<Refused, _>(t, start, accept, &limits(max), MAX_REDIRECTS, allow, &mut sink).map_err(|e| e.0)?;
    if got.head.status != 200 {
        return Err(format!("{} answered {}", hosts_name(url), got.head.status));
    }
    Ok(sink.bytes)
}

/// The host of an address as text for a message (never the path).
fn hosts_name(url: &str) -> String {
    Url::parse(url).map(|u| u.host().to_string()).unwrap_or_else(|_| "the server".to_string())
}

fn utf8(bytes: Vec<u8>, what: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| format!("{what} is not UTF-8 text"))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn now_seconds() -> Result<u64, String> {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).map_err(|_| "the clock is before 1970".to_string())
}

/// The calendar day (`YYYY-MM-DD`) of a count of days since 1970-01-01 (proleptic Gregorian calendar).
fn civil_date(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `--name value` pairs, each name from `allowed` at most once; anything else is an error.
fn options(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        if !allowed.contains(&name) {
            return Err("an unknown option".to_string());
        }
        let value = args.get(index + 1).filter(|v| !v.is_empty()).ok_or_else(|| format!("{name} needs a value"))?;
        if out.insert(name.to_string(), value.clone()).is_some() {
            return Err(format!("{name} is given twice"));
        }
        index += 2;
    }
    Ok(out)
}

fn read_capped(path: &Path, what: &str, max: u64) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|_| format!("{what} cannot be read"))?;
    if !meta.is_file() {
        return Err(format!("{what} is not a file"));
    }
    if meta.len() > max {
        return Err(format!("{what} is bigger than {max} bytes"));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("{what} cannot be read"))?;
    utf8(bytes, what)
}

fn write_file(path: &Path, bytes: &[u8], what: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| format!("the folder of {what} could not be made"))?;
    }
    std::fs::write(path, bytes).map_err(|_| format!("{what} could not be written"))
}

// ---- fetch ---------------------------------------------------------------------------------------------------------

/// What the fetch brings: Anthropic's model list (`None` without a key) and the Epoch table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    pub anthropic: Option<String>,
    pub epoch: String,
}

/// `GET /v1/models` with the key. The request holds a secret, so it goes through `fetch`, which refuses every redirect.
fn anthropic_models(t: &dyn Transport, key: &str) -> Result<String, String> {
    let url = host_of(ANTHROPIC_URL, &[ANTHROPIC_HOST])?;
    let accept = Header::new("Accept", b"application/json").map_err(net)?;
    let version = Header::new("anthropic-version", ANTHROPIC_VERSION.as_bytes()).map_err(net)?;
    let credential = Header::new("x-api-key", key.as_bytes()).map_err(|_| "the key cannot be sent as a header".to_string())?;
    let headers = [accept, version, credential];
    let request = Request { method: Method::Get, url: &url, headers: &headers, body: &[] };
    let mut sink = Collect::default();
    fetch(t, &request, &limits(MAX_ANSWER), &mut sink).map_err(net)?;
    if sink.status != 200 {
        return Err(format!("{ANTHROPIC_HOST} answered {}", sink.status));
    }
    let text = utf8(sink.bytes, "the model list")?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| "the model list is not JSON".to_string())?;
    if value.get("data").and_then(|d| d.as_array()).is_none() {
        return Err("the model list has no \"data\" list".to_string());
    }
    if value.get("has_more").and_then(|h| h.as_bool()) == Some(true) {
        return Err("the model list says there are more entries than one answer holds".to_string());
    }
    Ok(text)
}

fn epoch_table(t: &dyn Transport) -> Result<String, String> {
    let bytes = get_public(t, EPOCH_URL, "text/csv", &[EPOCH_HOST], MAX_ANSWER)?;
    let text = utf8(bytes, "the Epoch table")?;
    if text.trim().is_empty() {
        return Err("the Epoch table is empty".to_string());
    }
    Ok(text)
}

/// The inputs of the build. An empty (or blank) `key` means no key is given: Anthropic is not asked at all.
pub fn fetch_with(t: &dyn Transport, key: &str) -> Result<Sources, String> {
    let key = key.trim();
    let anthropic = if key.is_empty() { None } else { Some(anthropic_models(t, key)?) };
    let epoch = epoch_table(t)?;
    Ok(Sources { anthropic, epoch })
}

/// [`fetch_with`] and the two files in `out`: `epoch.csv`, and `models.json` only when Anthropic was asked (a stale
/// `models.json` of an earlier run is removed). `log` gets plain lines and never the key.
pub fn run_fetch(t: &dyn Transport, out: &Path, key: &str, log: &mut dyn Write) -> Result<(), String> {
    let sources = fetch_with(t, key)?;
    write_file(&out.join(FETCHED_EPOCH), sources.epoch.as_bytes(), "the Epoch table")?;
    let models = out.join(FETCHED_MODELS);
    match &sources.anthropic {
        Some(text) => {
            write_file(&models, text.as_bytes(), "the model list")?;
            let _ = writeln!(log, "xtask-catalog fetch: the model list and the Epoch table are in place");
        }
        None => {
            if models.exists() {
                std::fs::remove_file(&models).map_err(|_| "an old model list could not be removed".to_string())?;
            }
            let _ = writeln!(log, "xtask-catalog fetch: no key is given (O-2): only the Epoch table was fetched, nothing will be published");
        }
    }
    Ok(())
}

/// `fetch --out <dir>`. The one place the key variable is read.
pub fn fetch_cli(args: &[String]) -> u8 {
    let opts = match options(args, &["--out"]) {
        Ok(o) => o,
        Err(why) => return usage_error("fetch", &why, "fetch --out <dir>"),
    };
    let Some(out) = opts.get("--out") else { return usage_error("fetch", "--out is missing", "fetch --out <dir>") };
    let key = std::env::var(KEY_VARIABLE).unwrap_or_default();
    let transport = match system_transport(USER_AGENT) {
        Ok(t) => t,
        Err(why) => return failed("fetch", &net(why)),
    };
    match run_fetch(&transport, Path::new(out), &key, &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(why) => failed("fetch", &why),
    }
}

// ---- build ---------------------------------------------------------------------------------------------------------

/// What `build` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Built {
    /// A candidate of the whole sources was written.
    Candidate,
    /// There was no Anthropic list: the Epoch table and the hand table read, nothing was written.
    CheckedOnly,
}

/// A list of models in the shape of Anthropic's answer, made from the hand table: its first Anthropic id, only the
/// default effort, status `active`. It exists to check the other two sources when the key is missing; it is never
/// written out. A hand table that does not read gives an empty list, and `build` then names the hand table's problems.
fn list_from_hand(hand: &str) -> String {
    let mut data: Vec<serde_json::Value> = Vec::new();
    if let Ok(table) = hand.parse::<toml::Table>() {
        if let Some(models) = table.get("model").and_then(|m| m.as_array()) {
            for model in models {
                let id = model.get("anthropic").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|i| i.as_str());
                let effort = model.get("default_effort").and_then(|e| e.as_str());
                if let (Some(id), Some(effort)) = (id, effort) {
                    let mut levels = serde_json::Map::new();
                    levels.insert("supported".to_string(), serde_json::Value::Bool(true));
                    levels.insert(effort.to_string(), serde_json::json!({ "supported": true }));
                    data.push(serde_json::json!({
                        "id": id,
                        "capabilities": { "effort": serde_json::Value::Object(levels) },
                        "status": "active"
                    }));
                }
            }
        }
    }
    serde_json::json!({ "data": data }).to_string()
}

/// Builds the candidate from the files in `inp` (`epoch.csv`, `models.json` when there is one) and the hand table.
/// Without `models.json` the other two sources are only checked and `out` is removed, so nothing can be published.
pub fn run_build(inp: &Path, hand: &Path, out: &Path, rev: u32, today: &str, log: &mut dyn Write) -> Result<Built, Vec<String>> {
    let one = |why: String| vec![why];
    let epoch = read_capped(&inp.join(FETCHED_EPOCH), "the Epoch table", MAX_ANSWER).map_err(one)?;
    let hand_text = read_capped(hand, "the hand table", MAX_ANSWER).map_err(one)?;
    let models_path = inp.join(FETCHED_MODELS);
    if models_path.exists() {
        let models = read_capped(&models_path, "the model list", MAX_ANSWER).map_err(one)?;
        let text = crate::catalog::build(&models, &epoch, &hand_text, rev, today)?;
        write_file(out, text.as_bytes(), "the catalogue").map_err(one)?;
        let _ = writeln!(log, "xtask-catalog build: revision {rev} written");
        Ok(Built::Candidate)
    } else {
        crate::catalog::build(&list_from_hand(&hand_text), &epoch, &hand_text, rev, today)?;
        if out.exists() {
            std::fs::remove_file(out).map_err(|_| one("an old candidate could not be removed".to_string()))?;
        }
        let _ = writeln!(
            log,
            "xtask-catalog build: no Anthropic list, so the Epoch table and the hand table were only checked; nothing to publish"
        );
        Ok(Built::CheckedOnly)
    }
}

/// The revision of a candidate: whole minutes since 1970. It grows with every build and needs no state, and the update
/// gate only asks for a higher number than the one it holds.
pub fn revision_at(unix_seconds: u64) -> u32 {
    u32::try_from(unix_seconds / 60).unwrap_or(u32::MAX)
}

/// `build --in <dir> [--hand <file>] [--out <file>] [--rev <n>] [--today <YYYY-MM-DD>]`.
pub fn build_cli(args: &[String]) -> u8 {
    const USAGE: &str = "build --in <dir> [--hand <file>] [--out <file>] [--rev <n>] [--today <YYYY-MM-DD>]";
    let opts = match options(args, &["--in", "--hand", "--out", "--rev", "--today"]) {
        Ok(o) => o,
        Err(why) => return usage_error("build", &why, USAGE),
    };
    let Some(inp) = opts.get("--in") else { return usage_error("build", "--in is missing", USAGE) };
    let seconds = match now_seconds() {
        Ok(s) => s,
        Err(why) => return failed("build", &why),
    };
    let rev = match opts.get("--rev") {
        Some(text) => match text.parse::<u32>() {
            Ok(n) if n >= 1 => n,
            _ => return usage_error("build", "--rev is a whole number from 1", USAGE),
        },
        None => revision_at(seconds),
    };
    let today = match opts.get("--today") {
        Some(day) => day.clone(),
        None => civil_date(i64::try_from(seconds / 86_400).unwrap_or(0)),
    };
    let hand = PathBuf::from(opts.get("--hand").map_or(DEFAULT_HAND, String::as_str));
    let out = PathBuf::from(opts.get("--out").map_or(DEFAULT_CANDIDATE, String::as_str));
    match run_build(Path::new(inp), &hand, &out, rev, &today, &mut std::io::stdout()) {
        Ok(_) => 0,
        Err(problems) => {
            for problem in &problems {
                eprintln!("xtask-catalog build: {problem}");
            }
            1
        }
    }
}

// ---- publish -------------------------------------------------------------------------------------------------------

/// What the content of a candidate is, without what changes on every build: the lines `rev`, `generated` and the
/// `date` of each prior are left out. Equal fingerprints mean there is nothing new to publish.
pub fn fingerprint(text: &str) -> String {
    let mut kept = String::new();
    for line in text.lines() {
        if line.starts_with("rev = ") || line.starts_with("generated = ") || line.starts_with("date = ") {
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    sha256_hex(kept.as_bytes())
}

/// The name of the marker asset for a fingerprint.
pub fn marker_name(fingerprint: &str) -> String {
    format!("{MARKER_PREFIX}{}.txt", &fingerprint[..32.min(fingerprint.len())])
}

/// What `publish` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Published {
    /// The release already holds this content.
    Unchanged,
    /// These assets were uploaded, in this order.
    Uploaded(Vec<String>),
}

struct Asset {
    id: u64,
    name: String,
    digest: Option<String>,
}

struct Release {
    id: u64,
    assets: Vec<Asset>,
}

/// The GitHub API through the transport: the token is one header, the hosts are the two of [`PUBLISH_HOSTS`].
struct Api<'a> {
    t: &'a dyn Transport,
    bearer: String,
}

impl Api<'_> {
    fn call(&self, method: Method, url: &str, body: &[u8], content_type: Option<&str>) -> Result<(u16, Vec<u8>), String> {
        let url = host_of(url, &PUBLISH_HOSTS)?;
        let mut headers = vec![
            Header::new("Accept", b"application/vnd.github+json").map_err(net)?,
            Header::new("X-GitHub-Api-Version", b"2022-11-28").map_err(net)?,
            Header::new("Authorization", self.bearer.as_bytes()).map_err(|_| "the token cannot be sent as a header".to_string())?,
        ];
        if let Some(kind) = content_type {
            headers.push(Header::new("Content-Type", kind.as_bytes()).map_err(net)?);
        }
        let request = Request { method, url: &url, headers: &headers, body };
        let mut sink = Collect::default();
        fetch(self.t, &request, &limits(MAX_ANSWER), &mut sink).map_err(net)?;
        Ok((sink.status, sink.bytes))
    }
}

fn parse_release(bytes: &[u8]) -> Result<Release, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "the release answer is not JSON".to_string())?;
    let id = value.get("id").and_then(|i| i.as_u64()).ok_or("the release answer has no id")?;
    let mut assets = Vec::new();
    for item in value.get("assets").and_then(|a| a.as_array()).ok_or("the release answer has no asset list")? {
        let id = item.get("id").and_then(|i| i.as_u64()).ok_or("an asset has no id")?;
        let name = item.get("name").and_then(|n| n.as_str()).ok_or("an asset has no name")?.to_string();
        let digest = item
            .get("digest")
            .and_then(|d| d.as_str())
            .and_then(|d| d.strip_prefix("sha256:"))
            .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|d| d.to_ascii_lowercase());
        assets.push(Asset { id, name, digest });
    }
    Ok(Release { id, assets })
}

/// `owner/repo`: letters, digits, `.`, `_`, `-` on both sides.
fn valid_repository(repo: &str) -> bool {
    let plain = |part: &str| !part.is_empty() && part.len() <= 100 && part.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    matches!(repo.split_once('/'), Some((owner, name)) if plain(owner) && plain(name) && !name.contains('/'))
}

fn is_plain_asset_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Publishes `text` as `model-catalog.toml` of the release `catalog` of `repo`, with the marker asset and a new
/// `SHA256SUMS.txt` that lists every asset of the release (the data files of other tasks keep their line, taken from the
/// digest GitHub reports; an asset without a digest refuses the whole publish). Nothing is uploaded when the release
/// already holds the same content. The sums file is the last upload, so a stop half way leaves a release that the
/// fetching side refuses (no sums), and the next run repairs it.
pub fn publish_with(t: &dyn Transport, repo: &str, text: &str, token: &str) -> Result<Published, String> {
    if !valid_repository(repo) {
        return Err("the repository is owner/name".to_string());
    }
    if token.trim().is_empty() {
        return Err("no token is given".to_string());
    }
    if text.is_empty() || !text.contains("[[model]]") {
        return Err("the file is not a model catalogue".to_string());
    }
    let api = Api { t, bearer: format!("Bearer {}", token.trim()) };
    let base = format!("https://{GITHUB_API_HOST}/repos/{repo}");

    let (status, body) = api.call(Method::Get, &format!("{base}/releases/tags/{RELEASE_TAG}"), &[], None)?;
    let release = match status {
        200 => parse_release(&body)?,
        404 => {
            let create = serde_json::json!({
                "tag_name": RELEASE_TAG,
                "name": "Catalogue data",
                "body": "Data files that Skipper fetches once a day: the model catalogue, its checksum file. Not a program release.",
                "draft": false,
                "prerelease": true,
                "make_latest": "false"
            })
            .to_string();
            let (status, body) = api.call(Method::Post, &format!("{base}/releases"), create.as_bytes(), Some("application/json"))?;
            if status != 201 {
                return Err(format!("{GITHUB_API_HOST} did not create the release ({status})"));
            }
            parse_release(&body)?
        }
        other => return Err(format!("{GITHUB_API_HOST} answered {other} for the release")),
    };

    let print = fingerprint(text);
    let marker = marker_name(&print);
    let has = |name: &str| release.assets.iter().any(|a| a.name == name);
    if has(&marker) && has(MODEL_FILE) && has(SUMS_FILE) {
        return Ok(Published::Unchanged);
    }

    let marker_body = format!("content-sha256: {print}\n");
    let mut sums: BTreeMap<String, String> = BTreeMap::new();
    sums.insert(MODEL_FILE.to_string(), sha256_hex(text.as_bytes()));
    sums.insert(marker.clone(), sha256_hex(marker_body.as_bytes()));
    let mut stale: Vec<&Asset> = Vec::new();
    for asset in &release.assets {
        if asset.name == MODEL_FILE || asset.name == SUMS_FILE || asset.name.starts_with(MARKER_PREFIX) {
            stale.push(asset);
            continue;
        }
        let digest = asset.digest.as_ref().ok_or_else(|| {
            format!("the asset {} has no digest; a sums file that left it out would be wrong, so nothing was published", asset.name)
        })?;
        if !is_plain_asset_name(&asset.name) {
            return Err("an asset of the release has a name that a sums file cannot hold".to_string());
        }
        sums.insert(asset.name.clone(), digest.clone());
    }
    let mut sums_text = String::new();
    for (name, digest) in &sums {
        sums_text.push_str(&format!("{digest}  {name}\n"));
    }

    for asset in stale {
        let (status, _) = api.call(Method::Delete, &format!("{base}/releases/assets/{}", asset.id), &[], None)?;
        if status != 204 {
            return Err(format!("{GITHUB_API_HOST} did not remove an old asset ({status})"));
        }
    }
    let mut uploaded = Vec::new();
    for (name, bytes, kind) in [
        (MODEL_FILE.to_string(), text.as_bytes().to_vec(), "application/octet-stream"),
        (marker, marker_body.into_bytes(), "text/plain"),
        (SUMS_FILE.to_string(), sums_text.into_bytes(), "text/plain"),
    ] {
        let url = format!("https://{GITHUB_UPLOADS_HOST}/repos/{repo}/releases/{}/assets?name={name}", release.id);
        let (status, _) = api.call(Method::Post, &url, &bytes, Some(kind))?;
        if status != 201 {
            return Err(format!("{GITHUB_UPLOADS_HOST} did not take {name} ({status})"));
        }
        uploaded.push(name);
    }
    Ok(Published::Uploaded(uploaded))
}

/// `publish --file <file>`: needs `GITHUB_TOKEN`, `GITHUB_REPOSITORY` and `CATALOG_PUBLISH=yes` in the environment.
pub fn publish_cli(args: &[String]) -> u8 {
    const USAGE: &str = "publish --file <model-catalog.toml>";
    let opts = match options(args, &["--file"]) {
        Ok(o) => o,
        Err(why) => return usage_error("publish", &why, USAGE),
    };
    let Some(file) = opts.get("--file") else { return usage_error("publish", "--file is missing", USAGE) };
    if std::env::var(PUBLISH_VARIABLE).as_deref() != Ok("yes") {
        println!("xtask-catalog publish: {PUBLISH_VARIABLE} is not \"yes\" (O-2): nothing is published");
        return 0;
    }
    let text = match read_capped(Path::new(file), "the catalogue", crate::catalog::CATALOG_CAP as u64) {
        Ok(t) => t,
        Err(why) => return failed("publish", &why),
    };
    let token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
    let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    let transport = match system_transport(USER_AGENT) {
        Ok(t) => t,
        Err(why) => return failed("publish", &net(why)),
    };
    match publish_with(&transport, &repo, &text, &token) {
        Ok(Published::Unchanged) => {
            println!("xtask-catalog publish: the release already holds this content");
            0
        }
        Ok(Published::Uploaded(names)) => {
            println!("xtask-catalog publish: uploaded {}", names.join(", "));
            0
        }
        Err(why) => failed("publish", &why),
    }
}

// ---- the docs half -------------------------------------------------------------------------------------------------

/// `v2.1.312` or `2.1.312` as `2.1.312`, nothing else.
pub fn plain_version(tag: &str) -> Option<String> {
    let text = tag.strip_prefix('v').unwrap_or(tag);
    let parts: Vec<&str> = text.split('.').collect();
    let whole = |p: &&str| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit());
    (parts.len() == 3 && parts.iter().all(whole)).then(|| text.to_string())
}

/// The newest Claude Code version: the `tag_name` of its latest GitHub release. With a token the request is
/// authenticated (the shared address of a runner is rate limited without one); it goes through `fetch`, which follows no
/// redirect, so the token reaches `api.github.com` only.
fn newest_claude_version(t: &dyn Transport, token: &str) -> Result<String, String> {
    let url = host_of(CLAUDE_RELEASE_URL, &[GITHUB_API_HOST])?;
    let bearer = format!("Bearer {}", token.trim());
    let mut headers = vec![Header::new("Accept", b"application/vnd.github+json").map_err(net)?];
    if !token.trim().is_empty() {
        headers.push(Header::new("Authorization", bearer.as_bytes()).map_err(|_| "the token cannot be sent as a header".to_string())?);
    }
    let request = Request { method: Method::Get, url: &url, headers: &headers, body: &[] };
    let mut sink = Collect::default();
    fetch(t, &request, &limits(MAX_ANSWER), &mut sink).map_err(net)?;
    if sink.status != 200 {
        return Err(format!("{GITHUB_API_HOST} answered {} for the newest version", sink.status));
    }
    let value: serde_json::Value = serde_json::from_slice(&sink.bytes).map_err(|_| "the release answer is not JSON".to_string())?;
    let tag = value.get("tag_name").and_then(|t| t.as_str()).ok_or("the release answer has no tag_name")?;
    plain_version(tag).ok_or_else(|| "the newest tag is not a version like v2.1.312".to_string())
}

/// The docs pages, the schema and the newest version into `out`. Every file is
/// checked to be UTF-8 text and not empty; one failure writes nothing.
pub fn run_cc_fetch(t: &dyn Transport, out: &Path, token: &str, log: &mut dyn Write) -> Result<(), String> {
    let version = newest_claude_version(t, token)?;
    let mut files: Vec<(&str, String)> = Vec::new();
    for (name, url) in DOCS_FILES {
        let bytes = get_public(t, url, "text/markdown, application/json", &DOCS_HOSTS, MAX_DOC)?;
        let text = utf8(bytes, name)?;
        if text.trim().is_empty() {
            return Err(format!("{name} came back empty"));
        }
        files.push((name, text));
    }
    for (name, text) in &files {
        write_file(&out.join(name), text.as_bytes(), name)?;
    }
    write_file(&out.join(VERSION_FILE), format!("{version}\n").as_bytes(), VERSION_FILE)?;
    let _ = writeln!(log, "xtask-catalog cc-fetch: the docs and the schema for Claude Code {version} are in place");
    Ok(())
}

/// `cc-fetch --out <dir>`. The token (`GITHUB_TOKEN`, read-only in the job) only raises the rate limit.
pub fn cc_fetch_cli(args: &[String]) -> u8 {
    let opts = match options(args, &["--out"]) {
        Ok(o) => o,
        Err(why) => return usage_error("cc-fetch", &why, "cc-fetch --out <dir>"),
    };
    let Some(out) = opts.get("--out") else { return usage_error("cc-fetch", "--out is missing", "cc-fetch --out <dir>") };
    let token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
    let transport = match system_transport(USER_AGENT) {
        Ok(t) => t,
        Err(why) => return failed("cc-fetch", &net(why)),
    };
    match run_cc_fetch(&transport, Path::new(out), &token, &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(why) => failed("cc-fetch", &why),
    }
}

// ---- the entry ------------------------------------------------------------------------------------------------------

const USAGE: &str = "usage: xtask-catalog (fetch | build | publish | cc-fetch) ...";

fn usage_error(command: &str, why: &str, usage: &str) -> u8 {
    eprintln!("xtask-catalog {command}: {why}");
    eprintln!("usage: xtask-catalog {usage}");
    2
}

fn failed(command: &str, why: &str) -> u8 {
    eprintln!("xtask-catalog {command}: {why}");
    1
}

/// Runs the subcommand named by the first argument; exit code 2 for anything else.
pub fn run(args: &[String]) -> u8 {
    let Some((name, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    match name.as_str() {
        "fetch" => fetch_cli(rest),
        "build" => build_cli(rest),
        "publish" => publish_cli(rest),
        "cc-fetch" => cc_fetch_cli(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
