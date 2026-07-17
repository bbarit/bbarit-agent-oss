//! Minimal MCP (Model Context Protocol) client.
//!
//! Connects to stdio MCP servers declared in `.mcp.json` (project cwd) or
//! `<user_app_dir>/mcp.json`, lists their tools, and calls them. Each MCP tool is
//! exposed to the agent as `mcp__<server>__<tool>` and dispatched back here.
//!
//! Transport: newline-delimited JSON-RPC 2.0 over the server's stdin/stdout. A
//! reader thread per server feeds responses into a channel so requests can time
//! out instead of hanging. Servers are spawned lazily and kept alive for the
//! session (so tool-list and tool-calls reuse one process).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::config::AppConfig;
use crate::tools::ToolSpec;

/// Tool calls may legitimately run long (a browser navigation, a slow API).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Startup handshake (initialize + tools/list) must be snappy: it runs on the
/// user's turn path, and a server that can't answer these in seconds is broken.
const INIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct ServerConfig {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: i64,
    /// (tool name, input schema, description) as reported by the server.
    tools: Vec<(String, Value, String)>,
}

static SERVERS: OnceLock<Mutex<HashMap<String, Server>>> = OnceLock::new();

type ConfigCache = Option<(PathBuf, PathBuf, HashMap<String, ServerConfig>)>;
static CONFIG_CACHE: OnceLock<Mutex<ConfigCache>> = OnceLock::new();

fn config_cache() -> &'static Mutex<ConfigCache> {
    CONFIG_CACHE.get_or_init(|| Mutex::new(None))
}

fn servers() -> &'static Mutex<HashMap<String, Server>> {
    SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Servers that failed to start this session. Without this tombstone a broken
/// entry in .mcp.json re-pays the full spawn+handshake timeout on EVERY turn
/// (observed: a dead server added 2 minutes to each user prompt).
static FAILED_SERVERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn failed_servers() -> &'static Mutex<HashSet<String>> {
    FAILED_SERVERS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Forget spawn failures so a retry can happen after a config edit.
/// `/mcp reload` uses the stronger `reload_servers`, which also tears down
/// running servers.
pub fn reset_failed_servers() {
    failed_servers().lock().unwrap().clear();
}

/// Above this many MCP tools, full schemas are deferred: only a lightweight
/// name+description index plus `mcp_find_tools` is registered, and a tool's
/// schema enters the tool list once mcp_find_tools activates it. Keeps large
/// MCP fleets from eating the context on every request.
const LAZY_THRESHOLD: usize = 12;

pub const FIND_TOOLS_NAME: &str = "mcp_find_tools";

static ACTIVATED_TOOLS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn activated_tools() -> &'static Mutex<HashSet<String>> {
    ACTIVATED_TOOLS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Full reload for `/mcp reload`: kill every running server and drop all
/// session state (spawn-failure tombstones, lazy-tool activations) so the next
/// tool-spec build respawns everything from the current config. Without this,
/// editing a server's command/args/env or removing it from .mcp.json does
/// nothing until app restart. Returns how many running servers were stopped.
pub fn reload_servers() -> usize {
    *config_cache().lock().unwrap() = None;
    reset_failed_servers();
    activated_tools().lock().unwrap().clear();
    let mut map = servers().lock().unwrap();
    let stopped = map.len();
    for (_, mut server) in map.drain() {
        // Dropping a `Child` only detaches it — kill explicitly (and reap)
        // so the old process doesn't linger with its stale config.
        let _ = server.child.kill();
        let _ = server.child.wait();
    }
    stopped
}

/// Read `mcpServers` from `.mcp.json` (cwd) and `<user_app_dir>/mcp.json`.
/// Add (or replace) a stdio server entry in the project `.mcp.json` and
/// return the file path written. Creates the file when missing; preserves
/// any other keys/servers already in it.
pub fn add_server(
    config: &AppConfig,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<PathBuf> {
    let path = config.cwd.join(".mcp.json");
    let mut root: Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("{} is not valid JSON", path.display()))?,
        Err(_) => serde_json::json!({}),
    };
    if !root.is_object() {
        anyhow::bail!("{} must contain a JSON object", path.display());
    }
    let servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    if !servers.is_object() {
        anyhow::bail!("mcpServers in {} must be an object", path.display());
    }
    let mut entry = serde_json::json!({ "command": command });
    if !args.is_empty() {
        entry["args"] = serde_json::json!(args);
    }
    servers
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), entry);
    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&root)?))
        .with_context(|| format!("cannot write {}", path.display()))?;
    *config_cache().lock().unwrap() = None;
    Ok(path)
}

/// Remove a server entry from the project `.mcp.json`. Returns true when an
/// entry was actually removed.
pub fn remove_server(config: &AppConfig, name: &str) -> Result<bool> {
    let path = config.cwd.join(".mcp.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    let mut root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .map(|servers| servers.remove(name).is_some())
        .unwrap_or(false);
    if removed {
        std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&root)?))
            .with_context(|| format!("cannot write {}", path.display()))?;
        *config_cache().lock().unwrap() = None;
    }
    Ok(removed)
}

fn read_config(config: &AppConfig) -> HashMap<String, ServerConfig> {
    if let Some((cwd, user_app_dir, configs)) = config_cache().lock().unwrap().as_ref()
        && *cwd == config.cwd
        && *user_app_dir == config.user_app_dir
    {
        return configs.clone();
    }

    let configs = read_config_uncached(config);
    *config_cache().lock().unwrap() = Some((
        config.cwd.clone(),
        config.user_app_dir.clone(),
        configs.clone(),
    ));
    configs
}

fn read_config_uncached(config: &AppConfig) -> HashMap<String, ServerConfig> {
    let mut out = HashMap::new();
    // First entry wins on a name clash: project, then own user config, then
    // (when interop is on) Claude Code's and Codex's own configs, untouched.
    for path in [
        config.cwd.join(".mcp.json"),
        config.user_app_dir.join("mcp.json"),
    ] {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
            continue;
        };
        for (name, server) in parse_mcp_servers_json(&value) {
            out.entry(name).or_insert(server);
        }
    }
    if interop_enabled()
        && let Some(home) = dirs_next::home_dir()
    {
        if let Ok(text) = std::fs::read_to_string(home.join(".claude.json"))
            && let Ok(value) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        {
            for (name, server) in parse_mcp_servers_json(&value) {
                out.entry(name).or_insert(server);
            }
        }
        if let Ok(text) = std::fs::read_to_string(home.join(".codex").join("config.toml")) {
            for (name, server) in parse_codex_mcp_toml(&text) {
                out.entry(name).or_insert(server);
            }
        }
    }
    out
}

/// Claude Code / Codex interop: reuse their MCP servers and skills exactly as
/// configured there. OFF by default — bbarit-oss stays self-contained and does
/// not spend prompt tokens on other tools' skill libraries. Enable with
/// `BBARIT_INTEROP=1` (process env or the agent dotenv) or `/interop on`.
pub fn interop_enabled() -> bool {
    let value = std::env::var("BBARIT_INTEROP")
        .ok()
        .or_else(|| crate::config::agent_env_var("BBARIT_INTEROP"));
    matches!(
        value.as_deref().map(str::trim),
        Some("1") | Some("true") | Some("on")
    )
}

/// Extract stdio servers from an `mcpServers` JSON object (the shared shape of
/// project `.mcp.json`, our user `mcp.json`, and Claude Code's `~/.claude.json`).
/// Non-stdio transports and `disabled: true` entries are skipped.
fn parse_mcp_servers_json(value: &Value) -> Vec<(String, ServerConfig)> {
    let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(name, def)| {
            let command = def.get("command").and_then(Value::as_str)?;
            if def.get("disabled").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            if let Some(kind) = def.get("type").and_then(Value::as_str)
                && kind != "stdio"
            {
                return None;
            }
            let args = def
                .get("args")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let env = def
                .get("env")
                .and_then(Value::as_object)
                .map(|map| {
                    map.iter()
                        .filter_map(|(key, val)| {
                            val.as_str().map(|val| (key.clone(), val.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some((
                name.clone(),
                ServerConfig {
                    command: command.to_string(),
                    args,
                    env,
                },
            ))
        })
        .collect()
}

/// Extract stdio servers from Codex's `~/.codex/config.toml` `[mcp_servers.*]`
/// tables, exactly as Codex would run them.
fn parse_codex_mcp_toml(text: &str) -> Vec<(String, ServerConfig)> {
    let Ok(root) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(servers) = root.get("mcp_servers").and_then(|v| v.as_table()) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(name, def)| {
            let command = def.get("command")?.as_str()?.to_string();
            if def.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
                return None;
            }
            let args = def
                .get("args")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let env = def
                .get("env")
                .and_then(|v| v.as_table())
                .map(|map| {
                    map.iter()
                        .filter_map(|(key, val)| {
                            val.as_str().map(|val| (key.clone(), val.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some((name.clone(), ServerConfig { command, args, env }))
        })
        .collect()
}

impl Server {
    fn spawn(cfg: &ServerConfig) -> Result<Server> {
        let mut command = crate::spawn::no_window_command(&cfg.command);
        command
            .args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, val) in &cfg.env {
            command.env(key, val);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("spawn MCP server `{}`", cfg.command))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Accept both stdio framings seen in the wild: the MCP standard
            // (one JSON object per line) and legacy LSP-style
            // `Content-Length: N\r\n\r\n{json}` frames.
            let mut stdout = stdout;
            let mut buffer: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let read = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                buffer.extend_from_slice(&chunk[..read]);
                while let Some(value) = next_frame(&mut buffer) {
                    if tx.send(value).is_err() {
                        return;
                    }
                }
            }
        });
        let mut server = Server {
            child,
            stdin,
            rx,
            next_id: 0,
            tools: Vec::new(),
        };
        server.initialize()?;
        server.load_tools()?;
        Ok(server)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT)
    }

    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&msg)?)?;
        self.stdin.flush()?;
        // Wait in short slices so Esc interrupts a slow server handshake — a
        // broken/slow entry otherwise pins the turn for the full timeout while
        // the UI shows "cancelling…" and appears to ignore Esc.
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if crate::commands::cancel_requested() {
                bail!("MCP request `{method}` cancelled");
            }
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| anyhow!("MCP request `{method}` timed out"))?;
            let value = match self
                .rx
                .recv_timeout(remaining.min(Duration::from_millis(200)))
            {
                Ok(value) => value,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("MCP request `{method}` timed out")
                }
            };
            if value.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(err) = value.get("error") {
                    bail!("MCP `{method}` error: {err}");
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
            // A notification or a stray response — keep waiting for ours.
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&msg)?)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn initialize(&mut self) -> Result<()> {
        self.request_with_timeout(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "bbarit", "version": env!("CARGO_PKG_VERSION") }
            }),
            INIT_TIMEOUT,
        )?;
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    fn load_tools(&mut self) -> Result<()> {
        let result = self.request_with_timeout("tools/list", json!({}), INIT_TIMEOUT)?;
        if let Some(tools) = result.get("tools").and_then(Value::as_array) {
            for tool in tools {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let schema = tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object" }));
                let desc = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                self.tools.push((name, schema, desc));
            }
        }
        Ok(())
    }

    fn call(&mut self, tool: &str, args: &Value) -> Result<String> {
        let result = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": args.clone() }),
        )?;
        let mut text = String::new();
        if let Some(content) = result.get("content").and_then(Value::as_array) {
            for chunk in content {
                if let Some(part) = chunk.get("text").and_then(Value::as_str) {
                    text.push_str(part);
                    text.push('\n');
                }
            }
        }
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            bail!("MCP tool `{tool}` reported an error: {}", text.trim());
        }
        let text = text.trim();
        Ok(if text.is_empty() {
            result.to_string()
        } else {
            text.to_string()
        })
    }
}

/// Pop the next complete JSON-RPC frame off `buffer`, supporting both
/// newline-delimited JSON (MCP standard) and `Content-Length` (LSP-style)
/// framing. Returns None when no complete frame is buffered yet.
fn next_frame(buffer: &mut Vec<u8>) -> Option<Value> {
    loop {
        // Drop leading whitespace/newlines between frames.
        let skip = buffer
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        buffer.drain(..skip);
        if buffer.is_empty() {
            return None;
        }
        if buffer.starts_with(b"Content-Length:") || buffer.starts_with(b"content-length:") {
            let header_end = find_subslice(buffer, b"\r\n\r\n")?;
            let header = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
            let length: usize = header.lines().find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|rest| rest.trim().parse().ok())
            })?;
            let body_start = header_end + 4;
            if buffer.len() < body_start + length {
                return None; // wait for the rest of the body
            }
            let body: Vec<u8> = buffer
                .drain(..body_start + length)
                .skip(body_start)
                .collect();
            match serde_json::from_slice::<Value>(&body) {
                Ok(value) => return Some(value),
                Err(_) => continue,
            }
        }
        // Newline-delimited: need a full line.
        let line_end = buffer.iter().position(|&byte| byte == b'\n')?;
        let line: Vec<u8> = buffer.drain(..=line_end).collect();
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(text) {
            Ok(value) => return Some(value),
            Err(_) => continue, // stray log line on stdout — skip it
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// True for tool names this client owns.
pub fn is_mcp_tool(name: &str) -> bool {
    name.starts_with("mcp__")
}

/// Tool specs for every tool on every configured MCP server. Spawns servers
/// lazily; a server that fails to start is skipped (so a bad entry can't break
/// the whole turn).
pub fn mcp_tool_specs(config: &AppConfig) -> Vec<ToolSpec> {
    let configs = read_config(config);
    if configs.is_empty() {
        return Vec::new();
    }
    let mut map = servers().lock().unwrap();
    let mut specs = Vec::new();
    for (name, cfg) in &configs {
        if !map.contains_key(name) {
            // Esc mid-startup: stop spawning the remaining servers so the turn
            // ends promptly instead of paying every handshake timeout first.
            if crate::commands::cancel_requested() {
                break;
            }
            if failed_servers().lock().unwrap().contains(name) {
                continue;
            }
            match Server::spawn(cfg) {
                Ok(server) => {
                    map.insert(name.clone(), server);
                }
                Err(error) => {
                    // A cancelled handshake is not a broken server — leave it
                    // untombstoned so the next turn retries it normally.
                    if crate::commands::cancel_requested() {
                        break;
                    }
                    failed_servers().lock().unwrap().insert(name.clone());
                    crate::llm::emit_activity(&format!(
                        "⚠ MCP server `{name}` failed to start ({error:#}) — skipping it for \
                         this session. Fix .mcp.json and run /mcp reload to retry.\n"
                    ));
                    continue;
                }
            }
        }
        if let Some(server) = map.get(name) {
            for (tool, schema, desc) in &server.tools {
                let detail = if desc.is_empty() {
                    format!("[MCP:{name}] {tool}")
                } else {
                    format!("[MCP:{name}] {desc}")
                };
                specs.push(ToolSpec {
                    name: format!("mcp__{name}__{tool}"),
                    description: detail,
                    parameters: schema.clone(),
                    prompt_snippet: None,
                    prompt_guidelines: Vec::new(),
                });
            }
        }
    }
    if specs.len() <= LAZY_THRESHOLD {
        return specs;
    }
    // Lazy mode: full schemas only for activated tools; the rest are listed by
    // name+summary inside the mcp_find_tools description.
    let activated = activated_tools().lock().unwrap();
    let mut index = String::new();
    let mut kept = Vec::new();
    for spec in specs {
        if activated.contains(&spec.name) {
            kept.push(spec);
        } else {
            let summary: String = spec.description.chars().take(100).collect();
            index.push_str(&format!("\n- {}: {}", spec.name, summary));
        }
    }
    kept.push(ToolSpec {
        name: FIND_TOOLS_NAME.to_string(),
        description: format!(
            "Load deferred MCP tools. The tools below are available but their schemas are \
             not loaded — they CANNOT be called until you load them with this tool. Pass a \
             short keyword query; matching tools' full schemas are returned and become \
             callable. Deferred tools:{index}"
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "keywords to match against tool names/descriptions"}
            },
            "required": ["query"]
        }),
        prompt_snippet: None,
        prompt_guidelines: Vec::new(),
    });
    kept
}

/// Execute `mcp_find_tools`: match query tokens against deferred tool
/// names/descriptions, activate the matches, and return their schemas.
pub fn find_tools(config: &AppConfig, query: &str) -> Result<String> {
    let configs = read_config(config);
    let tokens: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    let map = servers().lock().unwrap();
    let mut scored = Vec::new();
    for name in configs.keys() {
        if let Some(server) = map.get(name) {
            for (tool, schema, desc) in &server.tools {
                let full = format!("mcp__{name}__{tool}");
                let haystack = format!("{full} {desc}").to_lowercase();
                let score = tokens.iter().filter(|t| haystack.contains(*t)).count();
                if score > 0 || tokens.is_empty() {
                    scored.push((score, full, schema.clone(), desc.clone()));
                }
            }
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(8);
    if scored.is_empty() {
        return Ok(format!(
            "No MCP tools match `{query}`. Try broader keywords."
        ));
    }
    let mut activated = activated_tools().lock().unwrap();
    let mut out = String::from("Loaded MCP tools (now callable):\n");
    for (_, full, schema, desc) in &scored {
        activated.insert(full.clone());
        out.push_str(&format!("\n## {full}\n{desc}\nInput schema: {schema}\n"));
    }
    Ok(out)
}

/// Dispatch an `mcp__<server>__<tool>` call to its server.
pub fn call_tool(config: &AppConfig, name: &str, args: &Value) -> Result<String> {
    let rest = name
        .strip_prefix("mcp__")
        .ok_or_else(|| anyhow!("not an MCP tool: {name}"))?;
    let (server_name, tool) = rest
        .split_once("__")
        .ok_or_else(|| anyhow!("malformed MCP tool name: {name}"))?;
    let mut map = servers().lock().unwrap();
    if !map.contains_key(server_name) {
        let configs = read_config(config);
        let cfg = configs
            .get(server_name)
            .ok_or_else(|| anyhow!("unknown MCP server `{server_name}` (not in .mcp.json)"))?;
        let server = Server::spawn(cfg)?;
        map.insert(server_name.to_string(), server);
    }
    let server = map
        .get_mut(server_name)
        .ok_or_else(|| anyhow!("MCP server `{server_name}` unavailable"))?;
    let result = server.call(tool, args);
    // A dead server (crashed mid-session) leaves a stale entry whose stdin is a
    // broken pipe — every later call would fail forever. Evict on failure so
    // the next call respawns it, and try once more right now.
    if result.is_err() {
        map.remove(server_name);
        let configs = read_config(config);
        if let Some(cfg) = configs.get(server_name)
            && let Ok(server) = Server::spawn(cfg)
        {
            map.insert(server_name.to_string(), server);
            return map.get_mut(server_name).unwrap().call(tool, args);
        }
    }
    result
}

/// Human-readable list of configured servers and their tools, for `/mcp`.
pub fn format_status(config: &AppConfig) -> String {
    let configs = read_config(config);
    if configs.is_empty() {
        return "No MCP servers configured. Add them to `.mcp.json` (\"mcpServers\": { \"name\": \
                { \"command\": \"...\", \"args\": [...] } })."
            .to_string();
    }
    let specs = mcp_tool_specs(config);
    let map = servers().lock().unwrap();
    let mut lines = vec!["MCP servers (.mcp.json):".to_string()];
    for name in configs.keys() {
        let connected = map.get(name).map(|s| s.tools.len());
        match connected {
            Some(count) => lines.push(format!("  ✓ {name} — {count} tool(s)")),
            None => lines.push(format!("  ✗ {name} — failed to start")),
        }
    }
    if !specs.is_empty() {
        lines.push(String::new());
        lines.push("Tools:".to_string());
        for spec in specs {
            lines.push(format!("  - {}", spec.name));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn interop_parses_claude_json_and_codex_toml_stdio_servers() {
        // Claude Code shape (~/.claude.json)
        let claude = serde_json::json!({
            "mcpServers": {
                "gemini": {"type": "stdio", "command": "npx",
                           "args": ["-y", "@x/mcp"], "env": {"K": "v"}},
                "sse-thing": {"type": "sse", "url": "https://x"},   // non-stdio → skipped
                "off": {"command": "node", "disabled": true}        // disabled → skipped
            }
        });
        let got = parse_mcp_servers_json(&claude);
        assert_eq!(got.len(), 1);
        let (name, cfg) = &got[0];
        assert_eq!(name, "gemini");
        assert_eq!(cfg.command, "npx");
        assert_eq!(cfg.args, vec!["-y".to_string(), "@x/mcp".to_string()]);
        assert_eq!(cfg.env, vec![("K".to_string(), "v".to_string())]);

        // Codex shape (~/.codex/config.toml)
        let codex = r#"
[mcp_servers.node_repl]
command = "node_repl"
args = ["--flag"]
[mcp_servers.node_repl.env]
X = "1"
[mcp_servers.disabled_one]
command = "nope"
enabled = false
"#;
        let mut got = parse_codex_mcp_toml(codex);
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "node_repl");
        assert_eq!(got[0].1.args, vec!["--flag".to_string()]);
        assert_eq!(got[0].1.env, vec![("X".to_string(), "1".to_string())]);
    }

    #[test]
    fn add_and_remove_server_roundtrip_in_project_mcp_json() {
        let dir = std::env::temp_dir().join(format!("bbarit-oss-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::AppConfig::for_test(dir.clone());

        let path = add_server(
            &config,
            "everything",
            "npx",
            &["-y".into(), "mcp-everything".into()],
        )
        .expect("add writes .mcp.json");
        assert_eq!(path, dir.join(".mcp.json"));
        let parsed = read_config(&config);
        let entry = parsed
            .get("everything")
            .expect("added server is readable back");
        assert_eq!(entry.command, "npx");
        assert_eq!(
            entry.args,
            vec!["-y".to_string(), "mcp-everything".to_string()]
        );

        // Adding again replaces (no dupes), other keys survive.
        add_server(&config, "everything", "node", &[]).unwrap();
        assert_eq!(
            read_config(&config).get("everything").unwrap().command,
            "node"
        );

        assert!(remove_server(&config, "everything").unwrap());
        assert!(!remove_server(&config, "everything").unwrap());
        assert!(!read_config(&config).contains_key("everything"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn next_frame_parses_newline_delimited_json() {
        let mut buffer = b"{\"id\":1}\n{\"id\":2}\n".to_vec();
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 1);
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 2);
        assert!(next_frame(&mut buffer).is_none());
    }

    #[test]
    fn next_frame_parses_content_length_framing() {
        let body = "{\"id\":7}";
        let mut buffer = format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes();
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 7);
        assert!(next_frame(&mut buffer).is_none());
    }

    #[test]
    fn next_frame_waits_for_partial_content_length_body() {
        let mut buffer = b"Content-Length: 8\r\n\r\n{\"id\"".to_vec();
        assert!(next_frame(&mut buffer).is_none());
        buffer.extend_from_slice(b":7}");
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 7);
    }

    #[test]
    fn next_frame_skips_stray_log_lines() {
        let mut buffer = b"[server] started\n{\"id\":3}\n".to_vec();
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 3);
    }

    /// A child that blocks reading stdin forever, standing in for a live server.
    fn spawn_stub_child() -> Child {
        let mut cmd = if cfg!(windows) {
            let mut cmd = std::process::Command::new("cmd");
            cmd.args(["/C", "findstr", "x"]);
            cmd
        } else {
            std::process::Command::new("cat")
        };
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn().unwrap()
    }

    #[test]
    fn reload_servers_kills_running_servers_and_clears_state() {
        let mut child = spawn_stub_child();
        let stdin = child.stdin.take().unwrap();
        let (_tx, rx) = std::sync::mpsc::channel();
        servers().lock().unwrap().insert(
            "stub".to_string(),
            Server {
                child,
                stdin,
                rx,
                next_id: 0,
                tools: Vec::new(),
            },
        );
        failed_servers()
            .lock()
            .unwrap()
            .insert("broken".to_string());
        activated_tools()
            .lock()
            .unwrap()
            .insert("mcp__stub__tool".to_string());

        let stopped = reload_servers();

        // Other unit tests exercise MCP discovery in parallel and may have
        // populated the process-global server map too. The contract under
        // test is that reload stops every server (including our stub), not
        // that this test owns the only server in the process.
        assert!(stopped >= 1);
        assert!(!servers().lock().unwrap().contains_key("stub"));
        assert!(!failed_servers().lock().unwrap().contains("broken"));
        assert!(
            !activated_tools()
                .lock()
                .unwrap()
                .contains("mcp__stub__tool")
        );
    }

    #[test]
    fn next_frame_handles_mixed_framings_in_one_stream() {
        let body = "{\"id\":1}";
        let mut buffer = format!(
            "Content-Length: {}\r\n\r\n{}{{\"id\":2}}\n",
            body.len(),
            body
        )
        .into_bytes();
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 1);
        assert_eq!(next_frame(&mut buffer).unwrap()["id"], 2);
    }
}
