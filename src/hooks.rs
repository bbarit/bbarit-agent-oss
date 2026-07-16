//! Settings-driven shell hooks (Claude-Code style) that run at lifecycle events.
//!
//! Config: `<cwd>/.bbarit/hooks.json` (project) and `<user_app_dir>/hooks.json`
//! (global). Both are merged; project entries run after global ones.
//!
//! ```json
//! {
//!   "PreToolUse":      [{ "matcher": "write|edit|bash", "command": "..." }],
//!   "PostToolUse":     [{ "matcher": "*",               "command": "..." }],
//!   "UserPromptSubmit":[{ "command": "..." }],
//!   "Stop":            [{ "command": "..." }]
//! }
//! ```
//!
//! Each hook command receives the event as JSON on stdin and runs in the project
//! cwd. Convention (mirrors Claude Code):
//!   - exit 0  → allow; anything printed to stdout is surfaced as a note / added
//!     to context (UserPromptSubmit).
//!   - exit ≠0 → for `PreToolUse` this BLOCKS the tool; stdout/stderr is the reason.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::config::AppConfig;

/// Outcome of firing a hook event.
pub struct HookOutcome {
    /// When true (PreToolUse only), the tool must NOT run.
    pub blocked: bool,
    /// Text to surface to the agent (block reason, or added context/notes).
    pub message: String,
}

impl HookOutcome {
    fn allow() -> Self {
        Self {
            blocked: false,
            message: String::new(),
        }
    }
}

/// Read and merge the hook config from the user and project files.
fn load(config: &AppConfig) -> Value {
    let mut merged = json!({});
    // The user-level hooks file is always trusted (the user wrote it). The
    // project-level file runs arbitrary shell on lifecycle events, so an
    // untrusted repo must NOT get to register hooks just by being opened —
    // gate it on project trust, mirroring extensions/skills/personas.
    let mut paths = vec![config.user_app_dir.join("hooks.json")];
    if config.project_trusted {
        paths.push(config.cwd.join(".bbarit").join("hooks.json"));
    }
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        for (event, entries) in object {
            let Some(entries) = entries.as_array() else {
                continue;
            };
            let slot = merged
                .as_object_mut()
                .unwrap()
                .entry(event.clone())
                .or_insert_with(|| json!([]));
            if let Some(list) = slot.as_array_mut() {
                list.extend(entries.iter().cloned());
            }
        }
    }
    merged
}

/// True if `matcher` (a `|`-separated list, or `*`/empty for "any") matches `name`.
fn matches(matcher: Option<&str>, name: &str) -> bool {
    match matcher.map(str::trim) {
        None | Some("") | Some("*") => true,
        Some(pattern) => pattern.split('|').any(|part| part.trim() == name),
    }
}

/// Run one hook command: feed `payload` on stdin, return (exit_ok, stdout+stderr).
fn run_command(config: &AppConfig, command: &str, payload: &Value) -> (bool, String) {
    let mut cmd = if cfg!(windows) {
        let mut c = crate::spawn::no_window_command("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let shell = config.shell_path.as_deref().unwrap_or("sh");
        let mut c = Command::new(shell);
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(&config.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => return (true, format!("(hook failed to start: {error})")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{payload}");
        // Drop stdin so a hook that reads to EOF doesn't block.
    }
    // Drain output on a thread and bound the wait: a hook that hangs (prompts,
    // waits on a lock, tails a log) must not freeze the whole agent turn — this
    // wait is synchronous and has no Esc path through it. 60s matches the
    // Claude Code hook convention; past that we kill and fail the hook.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(Ok(output)) => {
            let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let err = String::from_utf8_lossy(&output.stderr);
            if !err.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(err.trim());
            }
            (output.status.success(), text)
        }
        Ok(Err(error)) => (true, format!("(hook error: {error})")),
        // Timed out: the wait thread still owns `child`, so it will reap the
        // process when it finally exits. Report a failure (fail-closed for a
        // blocking PreToolUse — the caller treats non-success as a block).
        Err(_) => (
            false,
            "(hook timed out after 60s and was abandoned)".to_string(),
        ),
    }
}

/// Fire every hook registered for `event`. For `PreToolUse`, a non-zero exit
/// blocks (returns `blocked: true`). Other events just collect stdout as notes.
fn fire(config: &AppConfig, event: &str, payload: Value) -> HookOutcome {
    let cfg = load(config);
    let Some(entries) = cfg.get(event).and_then(Value::as_array) else {
        return HookOutcome::allow();
    };
    let tool = payload.get("tool").and_then(Value::as_str).unwrap_or("");
    let mut notes: Vec<String> = Vec::new();
    for entry in entries {
        let matcher = entry.get("matcher").and_then(Value::as_str);
        if !matches(matcher, tool) {
            continue;
        }
        let Some(command) = entry.get("command").and_then(Value::as_str) else {
            continue;
        };
        let (ok, out) = run_command(config, command, &payload);
        if !ok && event == "PreToolUse" {
            return HookOutcome {
                blocked: true,
                message: if out.is_empty() {
                    format!(
                        "Blocked by a PreToolUse hook (matcher `{}`).",
                        matcher.unwrap_or("*")
                    )
                } else {
                    out
                },
            };
        }
        if !out.is_empty() {
            notes.push(out);
        }
    }
    HookOutcome {
        blocked: false,
        message: notes.join("\n"),
    }
}

/// PreToolUse: may block a tool before it runs.
pub fn pre_tool_use(config: &AppConfig, tool: &str, args: &Value) -> HookOutcome {
    fire(
        config,
        "PreToolUse",
        json!({
            "event": "PreToolUse",
            "tool": tool,
            "arguments": args,
            "cwd": config.cwd.display().to_string(),
        }),
    )
}

/// PostToolUse: runs after a tool; stdout is surfaced as a note.
pub fn post_tool_use(config: &AppConfig, tool: &str, args: &Value, result: &str) -> String {
    fire(
        config,
        "PostToolUse",
        json!({
            "event": "PostToolUse",
            "tool": tool,
            "arguments": args,
            "result": result,
            "cwd": config.cwd.display().to_string(),
        }),
    )
    .message
}

/// UserPromptSubmit: stdout is added to the conversation as extra context.
pub fn user_prompt_submit(config: &AppConfig, prompt: &str) -> String {
    fire(
        config,
        "UserPromptSubmit",
        json!({
            "event": "UserPromptSubmit",
            "prompt": prompt,
            "cwd": config.cwd.display().to_string(),
        }),
    )
    .message
}

/// Stop: runs when a turn finishes; stdout is surfaced as a note.
pub fn stop(config: &AppConfig) -> String {
    fire(
        config,
        "Stop",
        json!({ "event": "Stop", "cwd": config.cwd.display().to_string() }),
    )
    .message
}

/// `/hooks` — list configured hooks.
pub fn format_status(config: &AppConfig) -> String {
    let cfg = load(config);
    let Some(object) = cfg.as_object().filter(|o| !o.is_empty()) else {
        return "No hooks configured. Add them to `.bbarit/hooks.json` (events: PreToolUse, \
                PostToolUse, UserPromptSubmit, Stop)."
            .to_string();
    };
    let mut lines = vec!["Hooks (.bbarit/hooks.json):".to_string()];
    for (event, entries) in object {
        let count = entries.as_array().map(|a| a.len()).unwrap_or(0);
        lines.push(format!("  {event}: {count} hook(s)"));
        if let Some(list) = entries.as_array() {
            for entry in list {
                let matcher = entry.get("matcher").and_then(Value::as_str).unwrap_or("*");
                let command = entry.get("command").and_then(Value::as_str).unwrap_or("");
                lines.push(format!("    - [{matcher}] {command}"));
            }
        }
    }
    lines.join("\n")
}
