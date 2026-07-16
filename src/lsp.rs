//! Minimal LSP (Language Server Protocol) client.
//!
//! Spawns a stdio language server per language on demand and keeps it alive in a
//! global registry so subsequent calls reuse one process (and its warm index).
//! Exposes precise symbol navigation — go-to-definition, find-references, hover,
//! document/workspace symbols, and diagnostics — to the agent as the `lsp` tool.
//!
//! Transport is JSON-RPC 2.0 with LSP's `Content-Length: N\r\n\r\n<body>` framing
//! (this is the one place it differs from the newline-delimited MCP client in
//! `mcp.rs`). A reader thread per server parses framed messages and feeds them
//! into a channel so requests can time out instead of hanging, and so async
//! `publishDiagnostics` notifications can be drained on demand.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

/// Most requests: 15s. Navigation requests can be the first thing that forces a
/// cold language server (e.g. rust-analyzer) to finish indexing, so they get 30s.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const NAV_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// A freshly spawned server (rust-analyzer especially) answers navigation
/// requests with an empty result until indexing finishes — retry within this
/// window instead of reporting "not found" on a cold server.
const INDEXING_GRACE: Duration = Duration::from_secs(20);

fn nav_request(server: &mut LspServer, method: &str, params: Value) -> Result<Value> {
    loop {
        let result = server.request(method, params.clone(), NAV_REQUEST_TIMEOUT)?;
        let empty = result.is_null() || result.as_array().is_some_and(|items| items.is_empty());
        if !empty || server.spawned_at.elapsed() > INDEXING_GRACE {
            return Ok(result);
        }
        std::thread::sleep(Duration::from_millis(800));
    }
}
/// How long to wait for `publishDiagnostics` after opening a document.
// Servers debounce re-analysis after a didChange and often publish an empty
// set first — wait long enough for the real report, but return as soon as a
// non-empty one lands. A clean file pays the full wait (no completion signal).
const DIAGNOSTICS_WAIT: Duration = Duration::from_secs(10);

/// A language server we know how to launch.
struct ServerSpec {
    /// Registry key — one server family serves several extensions (ts/tsx/js/jsx).
    key: &'static str,
    command: String,
    args: Vec<String>,
    /// LSP `languageId` sent in `textDocument/didOpen`.
    language_id: &'static str,
}

/// Map a file extension to a launchable server, or explain how to install it.
fn detect_server(ext: &str) -> Result<ServerSpec> {
    let spec = |key, command: &str, args: &[&str], language_id| ServerSpec {
        key,
        command: command.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        language_id,
    };
    match ext {
        "rs" => {
            require("rust-analyzer", "rustup component add rust-analyzer")?;
            Ok(spec("rust", "rust-analyzer", &[], "rust"))
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            require(
                "typescript-language-server",
                "npm install -g typescript-language-server typescript",
            )?;
            let language_id = match ext {
                "tsx" => "typescriptreact",
                "ts" => "typescript",
                "jsx" => "javascriptreact",
                _ => "javascript",
            };
            Ok(spec(
                "typescript",
                "typescript-language-server",
                &["--stdio"],
                language_id,
            ))
        }
        "py" => {
            if in_path("pyright-langserver") {
                Ok(spec("python", "pyright-langserver", &["--stdio"], "python"))
            } else if in_path("pylsp") {
                Ok(spec("python", "pylsp", &[], "python"))
            } else {
                bail!(
                    "no Python language server found. Install pyright \
                     (`npm install -g pyright`) or python-lsp-server (`pip install python-lsp-server`)."
                )
            }
        }
        "go" => {
            require("gopls", "go install golang.org/x/tools/gopls@latest")?;
            Ok(spec("go", "gopls", &[], "go"))
        }
        other => bail!(
            "lsp: no known language server for `.{other}` files \
             (supported: rs, ts/tsx/js/jsx, py, go)."
        ),
    }
}

fn require(command: &str, install_hint: &str) -> Result<()> {
    if in_path(command) {
        Ok(())
    } else {
        bail!("`{command}` is not on PATH. Install it with: {install_hint}")
    }
}

/// True when an executable named `name` exists on PATH.
fn in_path(name: &str) -> bool {
    let Ok(paths) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        if dir.join(name).is_file() {
            return true;
        }
        #[cfg(windows)]
        for ext in ["exe", "cmd", "bat"] {
            if dir.join(format!("{name}.{ext}")).is_file() {
                return true;
            }
        }
    }
    false
}

struct LspServer {
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: i64,
    /// Opened document uri -> (last version sent, content hash). The hash lets us
    /// skip a redundant didChange when the file hasn't changed between calls —
    /// re-sending unchanged text makes servers cancel in-flight requests
    /// (`-32801 content modified`).
    opened: HashMap<String, (i64, u64)>,
    language_id: &'static str,
    /// When the server process started — used to retry empty nav results while
    /// the server is still indexing (rust-analyzer answers `null` until then).
    spawned_at: Instant,
}

enum Response {
    Ok(Value),
    ContentModified,
}

static SERVERS: OnceLock<Mutex<HashMap<String, LspServer>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, LspServer>> {
    SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl LspServer {
    fn spawn(cwd: &Path, spec: &ServerSpec) -> Result<LspServer> {
        let mut child = crate::spawn::no_window_command(&spec.command)
            .args(&spec.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn language server `{}`", spec.command))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            // Stops on EOF or a malformed frame — the server is gone.
            while let Ok(Some(value)) = read_message(&mut reader) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let mut server = LspServer {
            child,
            stdin,
            rx,
            next_id: 0,
            opened: HashMap::new(),
            language_id: spec.language_id,
            spawned_at: Instant::now(),
        };
        server.initialize(cwd)?;
        Ok(server)
    }

    fn send(&mut self, msg: &Value) -> Result<()> {
        let body = serde_json::to_string(msg)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
            match self.await_response(id, method, deadline)? {
                Response::Ok(value) => return Ok(value),
                // rust-analyzer cancels requests with `-32801 content modified`
                // while it is still loading the workspace. That's transient:
                // pause briefly and re-issue until the deadline.
                Response::ContentModified => {
                    if Instant::now() >= deadline {
                        bail!("LSP request `{method}` timed out (server still indexing)");
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        }
    }

    fn await_response(&mut self, id: i64, method: &str, deadline: Instant) -> Result<Response> {
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| anyhow!("LSP request `{method}` timed out"))?;
            let value = self
                .rx
                .recv_timeout(remaining)
                .map_err(|_| anyhow!("LSP request `{method}` timed out"))?;
            if std::env::var("BBARIT_LSP_DEBUG").is_ok() {
                eprintln!(
                    "[lsp<-] {}",
                    &value.to_string()[..value.to_string().len().min(200)]
                );
            }
            // A server→client REQUEST (registerCapability, workDoneProgress/create…)
            // carries BOTH "method" and its own "id" — the id can collide with ours,
            // so it must never be taken as our response. Acknowledge it (void result)
            // so the server doesn't stall waiting on us.
            if value.get("method").is_some() {
                if let Some(server_id) = value.get("id").filter(|v| !v.is_null()).cloned() {
                    let _ =
                        self.send(&json!({ "jsonrpc": "2.0", "id": server_id, "result": null }));
                }
                continue;
            }
            if value.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(err) = value.get("error") {
                    if err.get("code").and_then(Value::as_i64) == Some(-32801) {
                        return Ok(Response::ContentModified);
                    }
                    bail!("LSP `{method}` error: {err}");
                }
                return Ok(Response::Ok(
                    value.get("result").cloned().unwrap_or(Value::Null),
                ));
            }
            // An unrelated/stale response — keep waiting for ours.
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn initialize(&mut self, cwd: &Path) -> Result<()> {
        let root = path_to_uri(cwd);
        self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root,
                "capabilities": {
                    "textDocument": {
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "definition": { "linkSupport": true },
                        "references": {},
                        "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                        "publishDiagnostics": {}
                    },
                    "workspace": { "symbol": {} }
                },
                "workspaceFolders": [{ "uri": root, "name": "root" }],
                "clientInfo": { "name": "bbarit", "version": env!("CARGO_PKG_VERSION") }
            }),
            NAV_REQUEST_TIMEOUT,
        )?;
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    /// Ensure `uri` is open with the current text. Sends didOpen the first time,
    /// didChange (full replace) afterwards so edits between calls are picked up.
    fn ensure_open(&mut self, uri: &str, text: &str) -> Result<()> {
        let hash = text_hash(text);
        match self.opened.get(uri).copied() {
            Some((_, prev_hash)) if prev_hash == hash => {
                // Already open and unchanged — sending didChange would make the
                // server cancel our request as "content modified".
            }
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": self.language_id,
                            "version": 1,
                            "text": text
                        }
                    }),
                )?;
                self.opened.insert(uri.to_string(), (1, hash));
            }
            Some((version, _)) => {
                let next = version + 1;
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": next },
                        "contentChanges": [{ "text": text }]
                    }),
                )?;
                self.opened.insert(uri.to_string(), (next, hash));
            }
        }
        Ok(())
    }

    /// Drain publishDiagnostics for `uri` for up to `DIAGNOSTICS_WAIT`, keeping the
    /// latest report (servers often send an empty set first, then the real one).
    fn collect_diagnostics(&mut self, uri: &str) -> Option<Value> {
        let deadline = Instant::now() + DIAGNOSTICS_WAIT;
        let mut latest = None;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let Ok(msg) = self.rx.recv_timeout(remaining) else {
                break;
            };
            // Acknowledge server→client requests here too — dropping them can
            // stall the server mid-analysis.
            if msg.get("method").is_some()
                && let Some(server_id) = msg.get("id").filter(|v| !v.is_null()).cloned()
            {
                let _ = self.send(&json!({ "jsonrpc": "2.0", "id": server_id, "result": null }));
                continue;
            }
            if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
                && let Some(params) = msg.get("params")
                && params.get("uri").and_then(Value::as_str) == Some(uri)
            {
                let non_empty = params
                    .get("diagnostics")
                    .and_then(Value::as_array)
                    .is_some_and(|items| !items.is_empty());
                latest = Some(params.clone());
                // The real (non-empty) report arrived — no need to wait out
                // the window that only exists for debounced re-analysis.
                if non_empty {
                    break;
                }
            }
        }
        latest
    }
}

/// Read one LSP-framed JSON-RPC message. `Ok(None)` on clean EOF.
fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break; // blank line terminates the header block
        }
        if let Some(len) = parse_content_length(header) {
            content_length = Some(len);
        }
    }
    let len = content_length.ok_or_else(|| anyhow!("LSP frame missing Content-Length"))?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(serde_json::from_slice(&buf)?))
}

/// Parse a `Content-Length: N` header line, case-insensitively.
fn parse_content_length(header: &str) -> Option<usize> {
    let (name, value) = header.split_once(':')?;
    if name.trim().eq_ignore_ascii_case("Content-Length") {
        value.trim().parse().ok()
    } else {
        None
    }
}

fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Convert 1-based editor coordinates to 0-based LSP coordinates.
fn to_lsp_position(line: u64, character: u64) -> (u64, u64) {
    (line.saturating_sub(1), character.saturating_sub(1))
}

/// Convert 0-based LSP coordinates back to 1-based editor coordinates.
fn from_lsp_position(line: u64, character: u64) -> (u64, u64) {
    (line + 1, character + 1)
}

/// The `lsp` tool entry point. See the ToolSpec in `tools.rs` for the contract.
pub fn execute(cwd: &Path, args: &Value) -> Result<String> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("lsp: 'action' is required (definition, references, hover, document_symbols, workspace_symbols, diagnostics)"))?;
    let file = args
        .get("file")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("lsp: 'file' is required"))?;

    // Canonicalize the root so it matches the canonicalized file URIs below —
    // on macOS temp_dir() is /var/... (a symlink) while files canonicalize to
    // /private/var/...; a mismatched rootUri makes the server treat every file
    // as outside the workspace and answer navigation with empty results.
    let cwd = &cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let abs = resolve(cwd, file);
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let spec = detect_server(&ext)?;
    // One server per (language, workspace root): the root is baked into the
    // handshake, so a server started for another project can't serve this one.
    let key = format!("{}::{}", spec.key, cwd.display());

    let mut reg = registry().lock().unwrap();
    if !reg.contains_key(&key) {
        let server = LspServer::spawn(cwd, &spec)?;
        reg.insert(key.clone(), server);
    }
    let server = reg
        .get_mut(&key)
        .ok_or_else(|| anyhow!("language server `{key}` unavailable"))?;

    let text = std::fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?;
    let uri = path_to_uri(&abs);
    server.ensure_open(&uri, &text)?;

    match action {
        "definition" | "references" | "hover" => {
            let line = args
                .get("line")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("lsp: '{action}' needs 'line' (1-based)"))?;
            let character = args.get("character").and_then(Value::as_u64).unwrap_or(1);
            let (lsp_line, lsp_char) = to_lsp_position(line, character);
            let position = json!({ "line": lsp_line, "character": lsp_char });
            match action {
                "definition" => {
                    let result = nav_request(
                        server,
                        "textDocument/definition",
                        json!({ "textDocument": { "uri": uri }, "position": position }),
                    )?;
                    Ok(format_locations(cwd, &result, "definition"))
                }
                "references" => {
                    let result = nav_request(
                        server,
                        "textDocument/references",
                        json!({
                            "textDocument": { "uri": uri },
                            "position": position,
                            "context": { "includeDeclaration": true }
                        }),
                    )?;
                    Ok(format_locations(cwd, &result, "references"))
                }
                _ => {
                    let result = nav_request(
                        server,
                        "textDocument/hover",
                        json!({ "textDocument": { "uri": uri }, "position": position }),
                    )?;
                    Ok(format_hover(&result))
                }
            }
        }
        "document_symbols" => {
            let result = server.request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
                REQUEST_TIMEOUT,
            )?;
            Ok(format_document_symbols(cwd, &abs, &result))
        }
        "workspace_symbols" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let result = nav_request(server, "workspace/symbol", json!({ "query": query }))?;
            Ok(format_symbol_information(cwd, &result))
        }
        "diagnostics" => {
            // rust-analyzer runs the compiler (flycheck) only on save — didChange
            // alone yields no type/borrow errors. The file is already on disk, so
            // a didSave is truthful and triggers the real check.
            server.notify(
                "textDocument/didSave",
                json!({ "textDocument": { "uri": uri } }),
            )?;
            let params = server.collect_diagnostics(&uri);
            Ok(format_diagnostics(cwd, &abs, params.as_ref()))
        }
        other => bail!(
            "lsp: unknown action `{other}` \
             (definition, references, hover, document_symbols, workspace_symbols, diagnostics)"
        ),
    }
}

/// Resolve a user-supplied file path to an absolute, canonical path.
fn resolve(cwd: &Path, file: &str) -> PathBuf {
    let p = Path::new(file);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    abs.canonicalize().unwrap_or(abs)
}

fn display_path(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Read one line (0-based) of a file, trimmed, for the `— content` suffix.
fn line_text(cache: &mut HashMap<String, Vec<String>>, path: &Path, line0: u64) -> Option<String> {
    let key = path.to_string_lossy().into_owned();
    let lines = cache
        .entry(key)
        .or_insert_with(|| match std::fs::read_to_string(path) {
            Ok(text) => text.lines().map(str::to_string).collect(),
            Err(_) => Vec::new(),
        });
    lines.get(line0 as usize).map(|l| l.trim().to_string())
}

/// Format a `Location`, `Location[]`, or `LocationLink[]` result.
fn format_locations(cwd: &Path, result: &Value, label: &str) -> String {
    let items: Vec<&Value> = match result {
        Value::Array(a) => a.iter().collect(),
        Value::Null => Vec::new(),
        single => vec![single],
    };
    if items.is_empty() {
        return format!("No {label} found.");
    }
    let mut cache = HashMap::new();
    let mut out = Vec::new();
    for item in items {
        // LocationLink uses targetUri/targetRange; Location uses uri/range.
        let uri = item
            .get("uri")
            .or_else(|| item.get("targetUri"))
            .and_then(Value::as_str);
        let range = item
            .get("range")
            .or_else(|| item.get("targetSelectionRange"))
            .or_else(|| item.get("targetRange"));
        let (Some(uri), Some(range)) = (uri, range) else {
            continue;
        };
        let start = range.get("start");
        let line0 = start
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let char0 = start
            .and_then(|s| s.get("character"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let path = uri_to_path(uri);
        let (line, col) = from_lsp_position(line0, char0);
        let content = line_text(&mut cache, &path, line0).unwrap_or_default();
        out.push(format!(
            "{}:{}:{} — {}",
            display_path(cwd, &path),
            line,
            col,
            content
        ));
    }
    if out.is_empty() {
        format!("No {label} found.")
    } else {
        out.join("\n")
    }
}

fn format_hover(result: &Value) -> String {
    let Some(contents) = result.get("contents") else {
        return "No hover information.".to_string();
    };
    let text = hover_text(contents);
    if text.trim().is_empty() {
        "No hover information.".to_string()
    } else {
        text.trim().to_string()
    }
}

fn hover_text(contents: &Value) -> String {
    match contents {
        Value::String(s) => s.clone(),
        // MarkupContent { kind, value } or MarkedString { language, value }.
        Value::Object(o) => o
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Value::Array(a) => a.iter().map(hover_text).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Format a `DocumentSymbol[]` (hierarchical) or `SymbolInformation[]` result.
fn format_document_symbols(cwd: &Path, path: &Path, result: &Value) -> String {
    let Some(items) = result.as_array() else {
        return "No symbols found.".to_string();
    };
    if items.is_empty() {
        return "No symbols found.".to_string();
    }
    // SymbolInformation has a `location`; DocumentSymbol has a `range`.
    if items.iter().any(|s| s.get("location").is_some()) {
        return format_symbol_information(cwd, result);
    }
    let display = display_path(cwd, path);
    let mut out = Vec::new();
    for symbol in items {
        push_document_symbol(&display, symbol, 0, &mut out);
    }
    if out.is_empty() {
        "No symbols found.".to_string()
    } else {
        out.join("\n")
    }
}

fn push_document_symbol(display: &str, symbol: &Value, depth: usize, out: &mut Vec<String>) {
    let name = symbol.get("name").and_then(Value::as_str).unwrap_or("?");
    let kind = symbol
        .get("kind")
        .and_then(Value::as_u64)
        .map(symbol_kind)
        .unwrap_or("");
    let start = symbol
        .get("selectionRange")
        .or_else(|| symbol.get("range"))
        .and_then(|r| r.get("start"));
    let line0 = start
        .and_then(|s| s.get("line"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let char0 = start
        .and_then(|s| s.get("character"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let (line, col) = from_lsp_position(line0, char0);
    let indent = "  ".repeat(depth);
    out.push(format!("{display}:{line}:{col} — {indent}{name} ({kind})"));
    if let Some(children) = symbol.get("children").and_then(Value::as_array) {
        for child in children {
            push_document_symbol(display, child, depth + 1, out);
        }
    }
}

/// Format a `SymbolInformation[]` result (workspace symbols, flat doc symbols).
fn format_symbol_information(cwd: &Path, result: &Value) -> String {
    let Some(items) = result.as_array() else {
        return "No symbols found.".to_string();
    };
    if items.is_empty() {
        return "No symbols found.".to_string();
    }
    let mut out = Vec::new();
    for symbol in items {
        let name = symbol.get("name").and_then(Value::as_str).unwrap_or("?");
        let kind = symbol
            .get("kind")
            .and_then(Value::as_u64)
            .map(symbol_kind)
            .unwrap_or("");
        let location = symbol.get("location");
        let uri = location.and_then(|l| l.get("uri")).and_then(Value::as_str);
        let start = location
            .and_then(|l| l.get("range"))
            .and_then(|r| r.get("start"));
        let line0 = start
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let char0 = start
            .and_then(|s| s.get("character"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let (line, col) = from_lsp_position(line0, char0);
        let path = uri.map(uri_to_path).unwrap_or_default();
        out.push(format!(
            "{}:{}:{} — {} ({})",
            display_path(cwd, &path),
            line,
            col,
            name,
            kind
        ));
    }
    out.join("\n")
}

fn format_diagnostics(cwd: &Path, path: &Path, params: Option<&Value>) -> String {
    let display = display_path(cwd, path);
    let Some(diagnostics) = params
        .and_then(|p| p.get("diagnostics"))
        .and_then(Value::as_array)
    else {
        return format!("No diagnostics for {display}.");
    };
    if diagnostics.is_empty() {
        return format!("No diagnostics for {display}.");
    }
    let mut out = Vec::new();
    for diag in diagnostics {
        let start = diag.get("range").and_then(|r| r.get("start"));
        let line0 = start
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let char0 = start
            .and_then(|s| s.get("character"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let (line, col) = from_lsp_position(line0, char0);
        let severity = diag
            .get("severity")
            .and_then(Value::as_u64)
            .map(diagnostic_severity)
            .unwrap_or("Info");
        let message = diag
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('\n', " ");
        out.push(format!("{display}:{line}:{col} — [{severity}] {message}"));
    }
    out.join("\n")
}

fn symbol_kind(kind: u64) -> &'static str {
    match kind {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Symbol",
    }
}

fn diagnostic_severity(severity: u64) -> &'static str {
    match severity {
        1 => "Error",
        2 => "Warning",
        3 => "Info",
        4 => "Hint",
        _ => "Info",
    }
}

/// Convert a filesystem path to a `file://` URI, percent-encoding as needed.
fn path_to_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    #[cfg(windows)]
    let raw = raw.replace('\\', "/");
    let mut uri = String::from("file://");
    #[cfg(windows)]
    uri.push('/');
    for ch in raw.chars() {
        match ch {
            '/' | ':' => uri.push(ch),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => uri.push(ch),
            _ => {
                for byte in ch.to_string().bytes() {
                    uri.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    uri
}

/// Convert a `file://` URI back to a filesystem path (percent-decoded).
fn uri_to_path(uri: &str) -> PathBuf {
    let rest = uri.strip_prefix("file://").unwrap_or(uri);
    // On Windows the path is `/C:/...`; drop the leading slash before the drive.
    #[cfg(windows)]
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    PathBuf::from(percent_decode(rest))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_content_length_framed_message() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = Cursor::new(frame.into_bytes());

        let message = read_message(&mut reader)
            .expect("frame should parse")
            .expect("a message, not EOF");
        assert_eq!(message["id"].as_i64(), Some(1));
        assert_eq!(message["result"]["ok"].as_bool(), Some(true));

        // Second read hits EOF cleanly.
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn converts_between_one_based_and_zero_based_positions() {
        // Editor 1-based -> LSP 0-based.
        assert_eq!(to_lsp_position(1, 1), (0, 0));
        assert_eq!(to_lsp_position(42, 5), (41, 4));
        // A defensive 0 stays at 0 rather than underflowing.
        assert_eq!(to_lsp_position(0, 0), (0, 0));
        // LSP 0-based -> editor 1-based (round trip).
        assert_eq!(from_lsp_position(0, 0), (1, 1));
        assert_eq!(from_lsp_position(41, 4), (42, 5));
        let (line, character) = (10u64, 3u64);
        let (l, c) = to_lsp_position(line, character);
        assert_eq!(from_lsp_position(l, c), (line, character));
    }

    #[test]
    fn parse_content_length_is_case_insensitive() {
        assert_eq!(parse_content_length("Content-Length: 42"), Some(42));
        assert_eq!(parse_content_length("content-length:7"), Some(7));
        assert_eq!(parse_content_length("Content-Type: application/json"), None);
    }

    #[test]
    #[ignore = "requires rust-analyzer on PATH; spawns a real server (slow)"]
    fn smoke_definition_with_rust_analyzer() {
        let dir = std::env::temp_dir().join("bbarit-lsp-smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn foo() -> u32 { 41 }\nfn main() { let x = foo(); println!(\"{x}\"); }\n",
        )
        .unwrap();
        let output = execute(
            &dir,
            &serde_json::json!({
                "action": "definition",
                "file": "src/main.rs",
                "line": 2,
                "character": 21
            }),
        )
        .unwrap();
        assert!(
            output.contains("main.rs:1"),
            "definition should point at line 1: {output}"
        );

        // The server is warm now — exercise the remaining actions on it too.
        let refs = execute(
            &dir,
            &serde_json::json!({
                "action": "references", "file": "src/main.rs", "line": 1, "character": 4
            }),
        )
        .unwrap();
        assert!(
            refs.contains("main.rs:2"),
            "references should include the call site: {refs}"
        );

        let hover = execute(
            &dir,
            &serde_json::json!({
                "action": "hover", "file": "src/main.rs", "line": 2, "character": 21
            }),
        )
        .unwrap();
        assert!(
            hover.contains("u32"),
            "hover should show foo's signature: {hover}"
        );

        let symbols = execute(
            &dir,
            &serde_json::json!({ "action": "document_symbols", "file": "src/main.rs" }),
        )
        .unwrap();
        assert!(
            symbols.contains("foo") && symbols.contains("main"),
            "symbols: {symbols}"
        );

        // Introduce a type error and confirm diagnostics surface it.
        std::fs::write(
            dir.join("src/main.rs"),
            "fn foo() -> u32 { \"oops\" }\nfn main() { let x = foo(); println!(\"{x}\"); }\n",
        )
        .unwrap();
        let diags = execute(
            &dir,
            &serde_json::json!({ "action": "diagnostics", "file": "src/main.rs" }),
        )
        .unwrap();
        assert!(
            diags.contains("mismatched") || diags.contains("expected"),
            "diagnostics should report the type error: {diags}"
        );
    }
}
