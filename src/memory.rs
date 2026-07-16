//! Automatic memory lifecycle (qwen-code style): recall relevant durable facts
//! when a turn starts, and extract new durable facts in the background when a
//! turn ends. Memories are plain `<slug>.md` files (frontmatter + body) under
//! `<user_app_dir>/memory`, with a `MEMORY.md` one-line index. No LLM is used
//! for recall — it is keyword overlap scoring; extraction runs a `--print`
//! sub-agent so it never blocks the main turn.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use serde_json::Value;

use crate::config::AppConfig;
use crate::session::{Message, Role};

/// Setting `BBARIT_AUTO_MEMORY=0` turns the whole feature off. Default: on.
fn enabled() -> bool {
    match std::env::var("BBARIT_AUTO_MEMORY") {
        Ok(value) => value.trim() != "0",
        Err(_) => true,
    }
}

fn memory_dir(config: &AppConfig) -> PathBuf {
    config.user_app_dir.join("memory")
}

#[derive(Clone)]
struct MemoryFile {
    name: String,
    description: String,
    body: String,
    /// Lowercased name + description + body, used for recall scoring.
    haystack: String,
}

fn parse_memory_file(text: &str) -> (BTreeMap<String, String>, String) {
    let mut fields = BTreeMap::new();
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"));
    let Some(rest) = rest else {
        return (fields, text.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (fields, text.to_string());
    };
    let front = &rest[..end];
    for line in front.lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }
    let body = rest[end..]
        .trim_start_matches(['-', '\n', '\r'])
        .trim_start()
        .to_string();
    (fields, body)
}

fn load_memory_files(config: &AppConfig) -> Vec<MemoryFile> {
    let dir = memory_dir(config);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("MEMORY.md") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let (fields, body) = parse_memory_file(&text);
        let name = fields.get("name").cloned().unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let description = fields.get("description").cloned().unwrap_or_default();
        let haystack = format!("{name} {description} {body}").to_lowercase();
        out.push(MemoryFile {
            name,
            description,
            body,
            haystack,
        });
    }
    out
}

/// `(built_at, memory_dir, files)` — parsed memory files cached per directory.
type MemoryCache = std::sync::Mutex<Option<(std::time::Instant, PathBuf, Vec<MemoryFile>)>>;

/// Like `load_memory_files`, but cached for a few seconds: memories change only
/// via `/memory` or auto-extraction, so re-reading and re-parsing every file on
/// each turn's recall is wasted I/O (same reasoning as the wiki/skills caches).
fn cached_memory_files(config: &AppConfig) -> Vec<MemoryFile> {
    static CACHE: std::sync::OnceLock<MemoryCache> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(None));
    let dir = memory_dir(config);
    if let Ok(guard) = cache.lock()
        && let Some((at, cached_dir, files)) = guard.as_ref()
        && *cached_dir == dir
        && at.elapsed() < std::time::Duration::from_secs(5)
    {
        return files.clone();
    }
    let files = load_memory_files(config);
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((std::time::Instant::now(), dir, files.clone()));
    }
    files
}

/// Split a prompt into distinct lowercase words of at least 3 characters — the
/// terms recall scores memories against.
fn prompt_terms(prompt: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in prompt
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
    {
        let word = raw.to_string();
        if !terms.contains(&word) {
            terms.push(word);
        }
    }
    terms
}

fn score_memory(mem: &MemoryFile, terms: &[String]) -> usize {
    terms
        .iter()
        .filter(|term| mem.haystack.contains(*term))
        .count()
}

/// Recall up to `max` stored memories most relevant to `prompt` (keyword overlap
/// only, no LLM). Bodies of the top matches are returned; memories scoring 0 are
/// excluded. Disabled when `BBARIT_AUTO_MEMORY=0`.
pub fn recall(config: &AppConfig, prompt: &str, max: usize) -> Vec<String> {
    if !enabled() || max == 0 {
        return Vec::new();
    }
    let terms = prompt_terms(prompt);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, MemoryFile)> = cached_memory_files(config)
        .into_iter()
        .map(|mem| (score_memory(&mem, &terms), mem))
        .filter(|(score, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(max)
        .map(|(_, mem)| cap_body(mem.body))
        .collect()
}

/// Recalled bodies ride along in every turn's (uncached) user message, so cap an
/// individual memory so one runaway note can't bloat the context; curated facts
/// sit far under this.
fn cap_body(body: String) -> String {
    const MAX: usize = 1500;
    if body.chars().count() <= MAX {
        return body;
    }
    let head: String = body.chars().take(MAX).collect();
    format!("{head}\n…(memory truncated)")
}

struct ExtractedMemory {
    mem_type: String,
    name: String,
    description: String,
    body: String,
}

/// Parse the sub-agent extraction output. Each durable fact is a single
/// `TYPE|name-slug|description|body` line; everything else (including `NONE`
/// and any framing text) is ignored.
fn parse_extraction(output: &str) -> Vec<ExtractedMemory> {
    let mut out = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("none") {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '|').map(str::trim).collect();
        if parts.len() != 4 {
            continue;
        }
        let mem_type = parts[0].to_lowercase();
        if !matches!(
            mem_type.as_str(),
            "user" | "feedback" | "project" | "reference"
        ) {
            continue;
        }
        let name = slugify(parts[1]);
        if name.is_empty() || parts[3].is_empty() {
            continue;
        }
        out.push(ExtractedMemory {
            mem_type,
            name,
            description: parts[2].to_string(),
            body: parts[3].to_string(),
        });
    }
    out
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').chars().take(64).collect()
}

fn write_memory(dir: &Path, mem: &ExtractedMemory) -> std::io::Result<()> {
    let path = dir.join(format!("{}.md", mem.name));
    let content = format!(
        "---\nname: {}\ndescription: {}\ntype: {}\n---\n\n{}\n",
        mem.name, mem.description, mem.mem_type, mem.body
    );
    fs::write(path, content)
}

fn rebuild_index(config: &AppConfig) {
    let dir = memory_dir(config);
    let mut lines = vec!["# Memory Index".to_string(), String::new()];
    let mut files = load_memory_files(config);
    files.sort_by(|a, b| a.name.cmp(&b.name));
    for mem in files {
        lines.push(format!("- {}: {}", mem.name, mem.description));
    }
    let _ = fs::write(dir.join("MEMORY.md"), lines.join("\n"));
}

/// Extract durable facts from a conversation delta in a background thread and
/// persist any it finds. Failures are swallowed so the main turn is never
/// disturbed. Disabled when `BBARIT_AUTO_MEMORY=0` or inside a sub-agent.
pub fn schedule_extract(config: &AppConfig, transcript_delta: String) {
    if !enabled() || crate::orchestrator::is_subagent() || transcript_delta.trim().is_empty() {
        return;
    }
    let config = config.clone();
    thread::spawn(move || {
        let Some(output) = run_extract_subagent(&config, &transcript_delta) else {
            return;
        };
        let extracted = parse_extraction(&output);
        if extracted.is_empty() {
            return;
        }
        let dir = memory_dir(&config);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        for mem in &extracted {
            let _ = write_memory(&dir, mem);
        }
        rebuild_index(&config);
    });
}

fn extraction_prompt(delta: &str) -> String {
    format!(
        "You maintain the long-term memory of a coding agent. From the conversation delta below, \
         extract ONLY durable facts that will still be useful in FUTURE sessions: user preferences, \
         feedback/corrections about how to work, or project constraints and decisions. Ignore \
         transient task state, one-off answers, and anything derivable from the code or git history.\n\n\
         Output 0 to 3 facts, each on its own line in EXACTLY this format:\n\
         TYPE|name-slug|one-line description|body\n\n\
         TYPE is one of: user, feedback, project, reference. name-slug is short kebab-case. body is \
         one or two sentences. Output no other text. If there is nothing durable, output exactly: NONE\n\n\
         Conversation delta:\n<delta>\n{delta}\n</delta>"
    )
}

/// Spawn a `--print --no-pick --no-session` sub-agent (same pattern as
/// [`crate::orchestrator::run_subagent`], but without its RESULT framing so the
/// extraction lines parse cleanly) and return its stdout.
fn run_extract_subagent(config: &AppConfig, delta: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let mut command = crate::spawn::no_window_command(&exe);
    command
        .env("BBARIT_AGENT_MODE", "1")
        .env("BBARIT_SUBAGENT", "1")
        // Memory extraction is pure text work — never pay for repo indexing.
        .env("BBARIT_AUTO_CONTEXT", "0")
        .arg("--print")
        .arg("--no-pick")
        .arg("--no-session")
        .arg("--provider")
        .arg(&config.provider);
    if let Some(model) = &config.model {
        command.arg("--model").arg(model);
    }
    command
        .arg(extraction_prompt(delta))
        .current_dir(&config.cwd);
    let output = command.output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

const EXTRACT_MIN_NEW_MESSAGES: usize = 4;
const DELTA_MAX_BYTES: usize = 16 * 1024;

fn cursor_file(config: &AppConfig) -> PathBuf {
    memory_dir(config).join(".extract-cursors.json")
}

fn read_cursor(config: &AppConfig, session_id: &str) -> usize {
    let text = match fs::read_to_string(cursor_file(config)) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| value.get(session_id).and_then(Value::as_u64))
        .map(|count| count as usize)
        .unwrap_or(0)
}

fn write_cursor(config: &AppConfig, session_id: &str, count: usize) {
    let path = cursor_file(config);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut value: Value = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        value = serde_json::json!({});
    }
    value[session_id] = serde_json::json!(count);
    let _ = fs::write(&path, value.to_string());
}

fn build_delta(messages: &[&Message]) -> String {
    let mut out = String::new();
    for message in messages {
        let role = match message.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => continue,
        };
        out.push_str(role);
        out.push_str(": ");
        out.push_str(message.content.trim());
        out.push_str("\n\n");
        if out.len() >= DELTA_MAX_BYTES {
            out.truncate(DELTA_MAX_BYTES);
            break;
        }
    }
    out
}

/// Called at the end of a completed turn: if enough new user/assistant messages
/// have accumulated since the last extraction, schedule a background extract of
/// the delta and advance the per-session cursor.
pub fn maybe_extract(config: &AppConfig, session_id: &str, messages: &[Message]) {
    if !enabled() || crate::orchestrator::is_subagent() {
        return;
    }
    let convo: Vec<&Message> = messages
        .iter()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant))
        .collect();
    let cursor = read_cursor(config, session_id);
    if convo.len() < cursor + EXTRACT_MIN_NEW_MESSAGES {
        return;
    }
    let delta = build_delta(&convo[cursor..]);
    write_cursor(config, session_id, convo.len());
    schedule_extract(config, delta);
}

/// `/memory` — list the index, or `/memory forget <name>` to delete one memory.
pub fn memory_command(config: &AppConfig, rest: &str) -> String {
    let rest = rest.trim();
    let dir = memory_dir(config);
    let (subcommand, arg) = match rest.split_once(char::is_whitespace) {
        Some((first, tail)) => (first, tail.trim()),
        None => (rest, ""),
    };
    // Word-boundary match: `/memory forgetful` must not become `forget ful`,
    // and unknown subcommands must not silently fall through to the listing.
    if subcommand == "reset" || subcommand == "clear" {
        let mut removed = 0;
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let is_index = path.file_name().and_then(|n| n.to_str()) == Some("MEMORY.md");
                    if !is_index && fs::remove_file(&path).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        let _ = fs::remove_file(dir.join("MEMORY.md"));
        let _ = fs::remove_file(dir.join(".extract-cursors.json"));
        format!(
            "Reset memory — removed {removed} memor{}.",
            if removed == 1 { "y" } else { "ies" }
        )
    } else if subcommand == "show" && !arg.is_empty() {
        let name = slugify(arg);
        match fs::read_to_string(dir.join(format!("{name}.md"))) {
            Ok(body) => body,
            Err(_) => format!("No memory named: {name}"),
        }
    } else if subcommand == "forget" {
        let name = slugify(arg);
        if name.is_empty() {
            return "usage: /memory forget <name>".to_string();
        }
        let path = dir.join(format!("{name}.md"));
        if path.exists() {
            let _ = fs::remove_file(&path);
            rebuild_index(config);
            format!("Forgot memory: {name}")
        } else {
            format!("No memory named: {name}")
        }
    } else if !rest.is_empty() {
        format!("unknown /memory subcommand '{subcommand}' — usage: /memory [forget <name>]")
    } else {
        let files = load_memory_files(config);
        if files.is_empty() {
            return "No memories stored yet.".to_string();
        }
        let mut lines = vec![format!("Memories ({}):", files.len())];
        for mem in files {
            lines.push(format!("- {}: {}", mem.name, mem.description));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_scores_by_keyword_overlap() {
        let higher = MemoryFile {
            name: "rust-build".to_string(),
            description: "prefers cargo check over full build".to_string(),
            body: "The user prefers running cargo check for fast feedback.".to_string(),
            haystack: "rust-build prefers cargo check over full build the user prefers running cargo check for fast feedback."
                .to_string(),
        };
        let lower = MemoryFile {
            name: "editor".to_string(),
            description: "uses neovim".to_string(),
            body: "editor preference".to_string(),
            haystack: "editor uses neovim editor preference".to_string(),
        };
        let terms = prompt_terms("please run cargo check for the rust build");
        assert!(score_memory(&higher, &terms) > score_memory(&lower, &terms));
        assert_eq!(score_memory(&lower, &terms), 0);
    }

    #[test]
    fn prompt_terms_drop_short_and_duplicate_words() {
        let terms = prompt_terms("Cargo cargo a an the check!");
        assert!(terms.contains(&"cargo".to_string()));
        assert!(terms.contains(&"check".to_string()));
        assert!(!terms.contains(&"an".to_string()));
        // "cargo" appears twice but is deduped.
        assert_eq!(terms.iter().filter(|t| *t == "cargo").count(), 1);
    }

    #[test]
    fn parse_extraction_keeps_valid_lines_only() {
        let output = "Here is what I found:\n\
             feedback|prefer-cargo-check|user likes cargo check|Run cargo check, not full builds.\n\
             garbage line without pipes\n\
             bogus|no-body|desc only\n\
             project|win-build|windows build lives elsewhere|Windows builds run on a separate machine.\n\
             == RESULT ==\n";
        let parsed = parse_extraction(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].mem_type, "feedback");
        assert_eq!(parsed[0].name, "prefer-cargo-check");
        assert_eq!(parsed[1].name, "win-build");
        assert_eq!(parsed[1].mem_type, "project");
    }

    #[test]
    fn parse_extraction_handles_none() {
        assert!(parse_extraction("NONE").is_empty());
        assert!(parse_extraction("none\n").is_empty());
    }

    #[test]
    fn slugify_produces_kebab_case() {
        assert_eq!(slugify("Prefer Cargo Check!"), "prefer-cargo-check");
        assert_eq!(slugify("  a  b  "), "a-b");
    }
}
