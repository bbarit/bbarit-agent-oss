use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::{AppConfig, PackageSpec};
use crate::providers::registry::ProviderRequestConfig;
use crate::providers::{Model, Provider};

#[derive(Debug, Clone)]
pub struct Extension {
    pub id: String,
    pub name: String,
    pub description: String,
    pub root: PathBuf,
    pub entry: Option<PathBuf>,
    pub commands: Vec<ExtensionCommand>,
    pub hooks: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExtensionCommand {
    pub extension_id: String,
    pub name: String,
    pub description: String,
    pub root: PathBuf,
    pub prompt: Option<String>,
    pub prompt_path: Option<PathBuf>,
    pub shell: Option<String>,
    pub runtime: bool,
}

#[derive(Debug, Clone)]
pub enum ExtensionAction {
    Prompt(String),
    Shell(String),
    Runtime(String),
}

#[derive(Debug, Clone)]
pub struct ResolvedExtensionCommand {
    pub extension_id: String,
    pub command_name: String,
    pub action: ExtensionAction,
}

#[derive(Debug, Clone)]
pub struct ExtensionProviderRegistration {
    pub provider: Provider,
    pub provider_api: String,
    pub replace_models: bool,
    pub models: Vec<Model>,
    pub headers: BTreeMap<String, String>,
    pub auth_header: Option<bool>,
    pub model_request_configs: BTreeMap<String, ProviderRequestConfig>,
}

#[derive(Debug, Clone)]
pub enum ExtensionProviderChange {
    Register(ExtensionProviderRegistration),
    Unregister(String),
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolOutput {
    pub text: String,
    pub terminate: bool,
}

#[derive(Debug, Clone)]
pub struct ExtensionToolCallPreflight {
    pub input: Value,
    pub blocked: bool,
    pub reason: Option<String>,
    pub outputs: Vec<Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ExtensionManifest {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    entry: Option<String>,
    main: Option<String>,
    commands: Option<CommandsManifest>,
    hooks: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CommandsManifest {
    List(Vec<CommandManifest>),
    Map(BTreeMap<String, CommandManifest>),
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CommandManifest {
    name: Option<String>,
    description: Option<String>,
    prompt: Option<String>,
    #[serde(alias = "prompt_path")]
    prompt_path: Option<String>,
    #[serde(alias = "shell")]
    command: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    name: Option<String>,
    description: Option<String>,
    main: Option<String>,
    module: Option<String>,
    #[serde(rename = "piAgent")]
    pi_agent: Option<ExtensionManifest>,
    pi: Option<PiPackageManifest>,
}

#[derive(Debug, Deserialize, Default)]
struct PiPackageManifest {
    extensions: Option<Vec<String>>,
}

pub fn load_extensions(config: &AppConfig) -> Result<Vec<Extension>> {
    let mut extensions = BTreeMap::new();
    if !config.no_extensions {
        for root in extension_base_dirs(config) {
            collect_extensions_from_base(&root, &mut extensions)?;
        }
    }
    for path in &config.extension_paths {
        collect_extension_path(path, &mut extensions)?;
    }
    if !config.no_extensions {
        for package in &config.packages {
            collect_package_extensions(package, &mut extensions)?;
        }
    }
    Ok(extensions.into_values().collect())
}

pub fn load_extensions_with_runtime_commands(config: &AppConfig) -> Result<Vec<Extension>> {
    let mut extensions = load_extensions(config)?;
    for extension in &mut extensions {
        append_runtime_commands(extension);
    }
    Ok(extensions)
}

pub fn resource_dirs(config: &AppConfig, kind: &str) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    let mut seen = BTreeSet::new();
    for extension in load_extensions(config)? {
        for candidate in [
            extension.root.join(kind),
            extension.root.join("resources").join(kind),
        ] {
            if seen.insert(candidate.clone()) {
                dirs.push(candidate);
            }
        }
        for candidate in discover_extension_resource_dirs(&extension, kind)? {
            if seen.insert(candidate.clone()) {
                dirs.push(candidate);
            }
        }
    }
    Ok(dirs)
}

fn discover_extension_resource_dirs(extension: &Extension, kind: &str) -> Result<Vec<PathBuf>> {
    let Some(entry) = extension.entry.as_ref() else {
        return Ok(Vec::new());
    };
    if !extension
        .hooks
        .iter()
        .any(|hook| hook == "resources_discover")
        || !is_node_runnable_extension_file(entry)
    {
        return Ok(Vec::new());
    }
    let value = run_node_extension_hook(
        extension,
        entry,
        "resources_discover",
        json!({
            "type": "resources_discover",
            "cwd": std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            "reason": "reload",
        }),
    )?;
    let field = match kind {
        "prompts" => "promptPaths",
        "skills" => "skillPaths",
        "themes" => "themePaths",
        _ => return Ok(Vec::new()),
    };
    let mut dirs = Vec::new();
    for result in extension_result_values(&value) {
        let Some(paths) = result.get(field).and_then(Value::as_array) else {
            continue;
        };
        for path in paths {
            let Some(path) = path.as_str().filter(|path| !path.trim().is_empty()) else {
                continue;
            };
            let candidate = PathBuf::from(path);
            if candidate.is_absolute() {
                dirs.push(candidate);
            } else {
                dirs.push(extension.root.join(candidate));
            }
        }
    }
    Ok(dirs)
}

pub fn resolve_command(
    config: &AppConfig,
    command_name: &str,
    raw_args: &str,
) -> Result<ResolvedExtensionCommand> {
    if command_name.trim().is_empty() {
        bail!("usage: /x <extension-command> [args]");
    }
    let args = parse_command_args(raw_args);
    let mut matches = load_extensions_with_runtime_commands(config)?
        .into_iter()
        .flat_map(|extension| extension.commands)
        .filter(|command| {
            command.name == command_name
                || format!("{}/{}", command.extension_id, command.name) == command_name
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        bail!("no extension command named {command_name}");
    }
    if matches.len() > 1 {
        bail!("ambiguous extension command {command_name}; use <extension>/<command>");
    }
    let command = matches.remove(0);
    let action = if command.runtime {
        ExtensionAction::Runtime(raw_args.to_string())
    } else if let Some(prompt) = command.prompt {
        ExtensionAction::Prompt(substitute_args(&prompt, &args))
    } else if let Some(path) = command.prompt_path {
        let path = if path.is_absolute() {
            path
        } else {
            command.root.join(path)
        };
        let prompt = fs::read_to_string(&path)
            .with_context(|| format!("failed to read extension prompt {}", path.display()))?;
        ExtensionAction::Prompt(substitute_args(&prompt, &args))
    } else if let Some(shell) = command.shell {
        ExtensionAction::Shell(substitute_shell_args(&shell, &args))
    } else {
        bail!("extension command {command_name} has no prompt, promptPath, or command");
    };
    Ok(ResolvedExtensionCommand {
        extension_id: command.extension_id,
        command_name: command.name,
        action,
    })
}

pub fn format_extension_list(config: &AppConfig) -> Result<String> {
    let extensions = load_extensions_with_runtime_commands(config)?;
    if extensions.is_empty() {
        return Ok("No extensions found.".to_string());
    }
    Ok(extensions
        .into_iter()
        .map(|extension| {
            format!(
                "{}\t{}\t{} command(s)\t{}",
                extension.id,
                extension.name,
                extension.commands.len(),
                extension.root.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn format_extension_detail(config: &AppConfig, id: &str) -> Result<String> {
    let extension = load_extensions_with_runtime_commands(config)?
        .into_iter()
        .find(|extension| extension.id == id || extension.name == id)
        .ok_or_else(|| anyhow!("no extension named {id}"))?;
    let mut lines = vec![
        format!("{} ({})", extension.name, extension.id),
        format!("Path: {}", extension.root.display()),
    ];
    if let Some(entry) = &extension.entry {
        lines.push(format!("Entry: {}", entry.display()));
    } else {
        lines.push("Entry: none".to_string());
    }
    if !extension.description.is_empty() {
        lines.push(format!("Description: {}", extension.description));
    }
    if extension.hooks.is_empty() {
        lines.push("Hooks: none".to_string());
    } else {
        lines.push(format!("Hooks: {}", extension.hooks.join(", ")));
    }
    if extension.commands.is_empty() {
        lines.push("Commands: none".to_string());
    } else {
        lines.push("Commands:".to_string());
        for command in extension.commands {
            let source = if command.runtime { " runtime" } else { "" };
            lines.push(format!(
                "  /x {}/{} (or /x {}){}\t{}",
                command.extension_id, command.name, command.name, source, command.description
            ));
        }
    }
    Ok(lines.join("\n"))
}

pub fn run_extension_hook(
    config: &AppConfig,
    extension_id: &str,
    event: &str,
    payload: Value,
) -> Result<Value> {
    if !EXTENSION_EVENT_NAMES.contains(&event) {
        bail!("unknown extension hook event: {event}");
    }
    let extension = load_extensions(config)?
        .into_iter()
        .find(|extension| extension.id == extension_id || extension.name == extension_id)
        .ok_or_else(|| anyhow!("no extension named {extension_id}"))?;
    let Some(entry) = extension.entry.as_ref() else {
        return Ok(json!({
            "available": false,
            "extensionId": extension.id,
            "event": event,
            "error": "extension has no executable entry"
        }));
    };
    if !is_node_runnable_extension_file(entry) {
        return Ok(json!({
            "available": false,
            "extensionId": extension.id,
            "event": event,
            "entry": entry.display().to_string(),
            "error": "only .js, .mjs, and .cjs extension runtime entries are executable in this Rust build"
        }));
    }
    run_node_extension_hook(&extension, entry, event, payload)
}

pub fn run_extension_event_hooks(
    config: &AppConfig,
    event: &str,
    payload: Value,
) -> Result<Vec<Value>> {
    if !EXTENSION_EVENT_NAMES.contains(&event) {
        bail!("unknown extension hook event: {event}");
    }
    let mut results = Vec::new();
    for extension in load_extensions(config)? {
        if !extension.hooks.iter().any(|hook| hook == event) {
            continue;
        }
        let Some(entry) = extension.entry.as_ref() else {
            continue;
        };
        if !is_node_runnable_extension_file(entry) {
            continue;
        }
        match run_node_extension_hook(&extension, entry, event, payload.clone()) {
            Ok(value) => results.push(value),
            Err(error) => results.push(json!({
                "ok": false,
                "available": true,
                "extensionId": extension.id,
                "event": event,
                "entry": entry.display().to_string(),
                "error": format!("{error:#}"),
            })),
        }
    }
    Ok(results)
}

pub fn run_tool_call_hooks(
    config: &AppConfig,
    tool_name: &str,
    tool_call_id: &str,
    input: &Value,
) -> Result<ExtensionToolCallPreflight> {
    let mut payload = json!({
        "type": "tool_call",
        "toolName": tool_name,
        "toolCallId": tool_call_id,
        "input": input,
    });
    let mut outputs = Vec::new();
    let mut blocked = false;
    let mut reason = None;

    for extension in load_extensions(config)? {
        if !extension.hooks.iter().any(|hook| hook == "tool_call") {
            continue;
        }
        let Some(entry) = extension.entry.as_ref() else {
            continue;
        };
        if !is_node_runnable_extension_file(entry) {
            continue;
        }

        let result = match run_node_extension_hook(&extension, entry, "tool_call", payload.clone())
        {
            Ok(value) => value,
            Err(error) => json!({
                "ok": false,
                "available": true,
                "extensionId": extension.id,
                "event": "tool_call",
                "entry": entry.display().to_string(),
                "error": format!("{error:#}"),
            }),
        };

        if let Some(updated_payload) = result.get("payload") {
            payload = updated_payload.clone();
        }

        if result.get("ok").and_then(Value::as_bool) == Some(false) {
            blocked = true;
            if reason.is_none() {
                reason = Some(
                    result
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call hook failed")
                        .to_string(),
                );
            }
        }

        for value in extension_result_values(&result) {
            if value.get("block").and_then(Value::as_bool).unwrap_or(false) {
                blocked = true;
                if reason.is_none() {
                    reason = value
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| Some("blocked by extension".to_string()));
                }
            }
        }

        outputs.push(result);
        if blocked {
            break;
        }
    }

    Ok(ExtensionToolCallPreflight {
        input: payload
            .get("input")
            .cloned()
            .unwrap_or_else(|| input.clone()),
        blocked,
        reason,
        outputs,
    })
}

pub fn extension_event_outputs_to_text(results: &[Value]) -> String {
    let mut lines = Vec::new();
    for result in results {
        let extension_id = result
            .get("extensionId")
            .and_then(Value::as_str)
            .unwrap_or("extension");
        if result.get("ok").and_then(Value::as_bool) == Some(false) {
            if let Some(error) = result.get("error").and_then(Value::as_str) {
                lines.push(format!("[{extension_id}] {error}"));
            }
            continue;
        }
        let text = runtime_outputs_to_text(result);
        if text != "Extension command completed." {
            for line in text.lines() {
                lines.push(format!("[{extension_id}] {line}"));
            }
        }
    }
    lines.join("\n")
}

pub fn transform_provider_request_payload(config: &AppConfig, payload: Value) -> Result<Value> {
    let mut current = payload;
    for result in run_extension_event_hooks(
        config,
        "before_provider_request",
        json!({
            "type": "before_provider_request",
            "payload": current,
        }),
    )? {
        for value in extension_result_values(&result) {
            current = value.clone();
        }
    }
    Ok(current)
}

fn extension_result_values(result: &Value) -> Vec<&Value> {
    let mut values = Vec::new();
    if let Some(outputs) = result.get("outputs").and_then(Value::as_array) {
        for output in outputs {
            if output.get("type").and_then(Value::as_str) == Some("result")
                && let Some(value) = output.get("value")
            {
                values.push(value);
            }
        }
    }
    values
}

pub fn run_extension_runtime_command(
    config: &AppConfig,
    extension_id: &str,
    command_name: &str,
    args: &str,
) -> Result<String> {
    let extension = load_extensions(config)?
        .into_iter()
        .find(|extension| extension.id == extension_id || extension.name == extension_id)
        .ok_or_else(|| anyhow!("no extension named {extension_id}"))?;
    let Some(entry) = extension.entry.as_ref() else {
        bail!("extension {extension_id} has no executable entry");
    };
    if !is_node_runnable_extension_file(entry) {
        bail!(
            "extension {extension_id} runtime command requires a .js, .mjs, .cjs, .ts, .tsx, or .jsx entry"
        );
    }
    let value = run_node_extension_runtime(
        &extension,
        entry,
        "command",
        Some(command_name),
        Some(args),
        None,
        None,
        None,
        Value::Null,
    )?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("extension command failed");
        bail!("{error}");
    }
    Ok(runtime_outputs_to_text(&value))
}

pub fn load_extension_tool_specs(config: &AppConfig) -> Result<Vec<crate::tools::ToolSpec>> {
    let mut specs = Vec::new();
    let mut seen = BTreeSet::new();
    for extension in load_extensions(config)? {
        let Some(entry) = extension.entry.as_ref() else {
            continue;
        };
        if !is_node_runnable_extension_file(entry) {
            continue;
        }
        let Ok(value) = run_node_extension_runtime(
            &extension,
            entry,
            "list_tools",
            None,
            None,
            None,
            None,
            None,
            Value::Null,
        ) else {
            continue;
        };
        let Some(tools) = value.get("tools").and_then(Value::as_array) else {
            continue;
        };
        for tool in tools {
            let Some(name) = tool.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !seen.insert(name.to_string()) {
                continue;
            }
            specs.push(crate::tools::ToolSpec {
                name: name.to_string(),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                parameters: tool
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                prompt_snippet: tool
                    .get("promptSnippet")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned),
                prompt_guidelines: parse_prompt_guidelines(tool.get("promptGuidelines")),
            });
        }
    }
    Ok(specs)
}

fn parse_prompt_guidelines(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => vec![value.trim().to_string()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn load_extension_provider_changes(config: &AppConfig) -> Result<Vec<ExtensionProviderChange>> {
    let mut changes = Vec::new();
    for extension in load_extensions(config)? {
        let Some(entry) = extension.entry.as_ref() else {
            continue;
        };
        if !is_node_runnable_extension_file(entry) {
            continue;
        }
        let Ok(value) = run_node_extension_runtime(
            &extension,
            entry,
            "list_providers",
            None,
            None,
            None,
            None,
            None,
            Value::Null,
        ) else {
            continue;
        };
        for provider_id in value
            .get("unregisteredProviders")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|provider_id| !provider_id.is_empty())
        {
            changes.push(ExtensionProviderChange::Unregister(provider_id.to_string()));
        }
        let Some(providers) = value.get("providers").and_then(Value::as_array) else {
            continue;
        };
        for provider_value in providers {
            if let Some(registration) = parse_extension_provider(provider_value) {
                changes.push(ExtensionProviderChange::Register(registration));
            }
        }
    }
    Ok(changes)
}

pub fn run_extension_provider_stream_simple(
    config: &AppConfig,
    provider_name: &str,
    payload: Value,
) -> Result<Option<Value>> {
    for extension in load_extensions(config)? {
        let Some(entry) = extension.entry.as_ref() else {
            continue;
        };
        if !is_node_runnable_extension_file(entry) {
            continue;
        }
        let Ok(value) = run_node_extension_runtime(
            &extension,
            entry,
            "list_providers",
            None,
            None,
            None,
            None,
            None,
            Value::Null,
        ) else {
            continue;
        };
        let has_stream_provider = value
            .get("providers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|provider| {
                provider.get("id").and_then(Value::as_str) == Some(provider_name)
                    && provider
                        .get("config")
                        .and_then(|config| config.get("streamSimple"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            });
        if !has_stream_provider {
            continue;
        }
        let value = run_node_extension_runtime(
            &extension,
            entry,
            "provider_stream_simple",
            None,
            None,
            None,
            None,
            None,
            json!({
                "providerName": provider_name,
                "model": payload.get("model").cloned().unwrap_or(Value::Null),
                "context": payload.get("context").cloned().unwrap_or(Value::Null),
                "options": payload.get("options").cloned().unwrap_or(Value::Null),
            }),
        )?;
        if value.get("ok").and_then(Value::as_bool) == Some(false) {
            let error = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("extension provider streamSimple failed");
            bail!("{error}");
        }
        return Ok(Some(value));
    }
    Ok(None)
}

pub fn run_extension_tool(
    config: &AppConfig,
    tool_name: &str,
    tool_call_id: &str,
    args: &Value,
) -> Result<Option<ExtensionToolOutput>> {
    for extension in load_extensions(config)? {
        let Some(entry) = extension.entry.clone() else {
            continue;
        };
        if !is_node_runnable_extension_file(&entry) {
            continue;
        }
        let Ok(value) = run_node_extension_runtime(
            &extension,
            &entry,
            "list_tools",
            None,
            None,
            None,
            None,
            None,
            Value::Null,
        ) else {
            continue;
        };
        let has_tool = value
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name));
        if !has_tool {
            continue;
        }
        return run_extension_tool_entry(extension, entry, tool_name, tool_call_id, args).map(Some);
    }
    Ok(None)
}

fn run_extension_tool_entry(
    extension: Extension,
    entry: PathBuf,
    tool_name: &str,
    tool_call_id: &str,
    args: &Value,
) -> Result<ExtensionToolOutput> {
    let value = run_node_extension_runtime(
        &extension,
        &entry,
        "tool",
        None,
        None,
        None,
        Some(tool_name),
        None,
        json!({
            "toolCallId": tool_call_id,
            "arguments": args,
        }),
    )?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("extension tool failed");
        bail!("{error}");
    }
    Ok(ExtensionToolOutput {
        text: runtime_outputs_to_text(&value),
        terminate: extension_tool_terminates(&value),
    })
}

fn extension_tool_terminates(value: &Value) -> bool {
    value
        .get("outputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|output| output.get("type").and_then(Value::as_str) == Some("result"))
        .any(|output| {
            output
                .get("raw")
                .or_else(|| output.get("value"))
                .and_then(|raw| raw.get("terminate"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
}

pub fn load_extension_shortcuts(config: &AppConfig) -> Result<Vec<ExtensionShortcutInfo>> {
    let mut shortcuts = Vec::new();
    let mut seen = BTreeSet::new();
    for extension in load_extensions(config)? {
        let Some(entry) = extension.entry.as_ref() else {
            continue;
        };
        if !is_node_runnable_extension_file(entry) {
            continue;
        }
        let Ok(value) = run_node_extension_runtime(
            &extension,
            entry,
            "list_shortcuts",
            None,
            None,
            None,
            None,
            None,
            Value::Null,
        ) else {
            continue;
        };
        let Some(items) = value.get("shortcuts").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(shortcut) = item.get("shortcut").and_then(Value::as_str) else {
                continue;
            };
            if !seen.insert(shortcut.to_string()) {
                continue;
            }
            shortcuts.push(ExtensionShortcutInfo {
                extension_id: extension.id.clone(),
                shortcut: shortcut.to_string(),
                description: item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    Ok(shortcuts)
}

pub fn run_extension_shortcut(config: &AppConfig, shortcut: &str) -> Result<Option<String>> {
    let mut matches = Vec::new();
    for extension in load_extensions(config)? {
        let Some(entry) = extension.entry.clone() else {
            continue;
        };
        if !is_node_runnable_extension_file(&entry) {
            continue;
        }
        let Ok(value) = run_node_extension_runtime(
            &extension,
            &entry,
            "list_shortcuts",
            None,
            None,
            None,
            None,
            None,
            Value::Null,
        ) else {
            continue;
        };
        let has_shortcut = value
            .get("shortcuts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|item| item.get("shortcut").and_then(Value::as_str) == Some(shortcut));
        if has_shortcut {
            matches.push((extension, entry));
        }
    }
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() > 1 {
        bail!("ambiguous extension shortcut {shortcut}; multiple extensions registered it");
    }
    let (extension, entry) = matches.remove(0);
    let value = run_node_extension_runtime(
        &extension,
        &entry,
        "shortcut",
        None,
        None,
        None,
        None,
        Some(shortcut),
        Value::Null,
    )?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("extension shortcut failed");
        bail!("{error}");
    }
    Ok(Some(runtime_outputs_to_text(&value)))
}

#[derive(Debug, Clone)]
pub struct ExtensionShortcutInfo {
    pub extension_id: String,
    pub shortcut: String,
    pub description: String,
}

fn parse_extension_provider(value: &Value) -> Option<ExtensionProviderRegistration> {
    let id = value.get("id")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }
    let config = value.get("config").unwrap_or(&Value::Null);
    let name = config
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_string();
    let base_url = config
        .get("baseUrl")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string);
    let provider_api = config
        .get("api")
        .and_then(Value::as_str)
        .unwrap_or("openai-completions")
        .to_string();
    let (api_key, mut api_key_env) = parse_extension_provider_api_key(id, config);
    if api_key.is_none() && api_key_env.is_empty() {
        api_key_env.push(format!(
            "BBARIT_PROVIDER_{}_API_KEY",
            id.to_uppercase().replace('-', "_")
        ));
    }
    let models_value = config.get("models").and_then(Value::as_array);
    let replace_models = models_value.is_some();
    let headers = parse_headers(config.get("headers"));
    let auth_header = config.get("authHeader").and_then(Value::as_bool);
    let mut models = Vec::new();
    let mut model_request_configs = BTreeMap::new();
    for model_value in models_value.into_iter().flatten() {
        let Some(model_id) = model_value.get("id").and_then(Value::as_str) else {
            continue;
        };
        if model_id.trim().is_empty() {
            continue;
        }
        let model_id = model_id.to_string();
        let model_headers = parse_headers(model_value.get("headers"));
        let model_auth_header = model_value.get("authHeader").and_then(Value::as_bool);
        if !model_headers.is_empty() || model_auth_header.is_some() {
            model_request_configs.insert(
                model_id.clone(),
                ProviderRequestConfig {
                    headers: model_headers,
                    auth_header: model_auth_header,
                },
            );
        }
        models.push(Model {
            id: model_id.to_string(),
            name: model_value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&model_id)
                .to_string(),
            api: model_value
                .get("api")
                .and_then(Value::as_str)
                .unwrap_or(&provider_api)
                .to_string(),
            provider: id.to_string(),
            base_url: model_value
                .get("baseUrl")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string)
                .or_else(|| base_url.clone()),
            reasoning: model_value
                .get("reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            context_window: model_value
                .get("contextWindow")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            max_tokens: model_value
                .get("maxTokens")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
        });
    }
    Some(ExtensionProviderRegistration {
        provider: Provider {
            id: id.to_string(),
            name,
            base_url,
            api_key,
            api_key_env,
        },
        provider_api,
        replace_models,
        models,
        headers,
        auth_header,
        model_request_configs,
    })
}

fn parse_headers(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    if key.trim().is_empty() {
                        return None;
                    }
                    let value = value
                        .as_str()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| value.to_string());
                    Some((key.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_extension_provider_api_key(
    provider_id: &str,
    config: &Value,
) -> (Option<String>, Vec<String>) {
    let Some(raw) = config.get("apiKey").and_then(Value::as_str) else {
        return (None, Vec::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, Vec::new());
    }
    if let Some(name) = trimmed
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        && !name.trim().is_empty()
    {
        return (None, vec![name.trim().to_string()]);
    }
    if let Some(name) = trimmed.strip_prefix('$')
        && !name.trim().is_empty()
    {
        return (None, vec![name.trim().to_string()]);
    }
    if trimmed == "true" {
        return (
            None,
            vec![format!(
                "BBARIT_PROVIDER_{}_API_KEY",
                provider_id.to_uppercase().replace('-', "_")
            )],
        );
    }
    (Some(raw.to_string()), Vec::new())
}

fn append_runtime_commands(extension: &mut Extension) {
    let Some(entry) = extension.entry.as_ref() else {
        return;
    };
    if !is_node_runnable_extension_file(entry) {
        return;
    }
    let Ok(value) = run_node_extension_runtime(
        extension,
        entry,
        "list_commands",
        None,
        None,
        None,
        None,
        None,
        Value::Null,
    ) else {
        return;
    };
    let Some(commands) = value.get("commands").and_then(Value::as_array) else {
        return;
    };
    let existing = extension
        .commands
        .iter()
        .map(|command| command.name.clone())
        .collect::<BTreeSet<_>>();
    for command in commands {
        let Some(name) = command.get("name").and_then(Value::as_str) else {
            continue;
        };
        if existing.contains(name) {
            continue;
        }
        extension.commands.push(ExtensionCommand {
            extension_id: extension.id.clone(),
            name: name.to_string(),
            description: command
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            root: extension.root.clone(),
            prompt: None,
            prompt_path: None,
            shell: None,
            runtime: true,
        });
    }
}

fn is_node_runnable_extension_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx")
    )
}

fn run_node_extension_hook(
    extension: &Extension,
    entry: &Path,
    event: &str,
    payload: Value,
) -> Result<Value> {
    run_node_extension_runtime(
        extension,
        entry,
        "hook",
        None,
        None,
        Some(event),
        None,
        None,
        payload,
    )
}

thread_local! {
    /// Live context snapshot (JSON) exposed to extensions as api.getModel() etc.
    /// Set per turn from the agent loop on the same thread the runtime runs on.
    static EXTENSION_CONTEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Set the JSON context snapshot extensions can read (model, cwd, trust, …).
pub fn set_extension_context(json: String) {
    EXTENSION_CONTEXT.with(|cell| *cell.borrow_mut() = json);
}

fn extension_context() -> String {
    EXTENSION_CONTEXT.with(|cell| {
        let value = cell.borrow();
        if value.is_empty() {
            "null".to_string()
        } else {
            value.clone()
        }
    })
}

// The mode + per-mode selector args mirror the Node wrapper's dispatch table;
// folding them into an invocation enum is tracked in ARCHITECTURE.md.
#[allow(clippy::too_many_arguments)]
fn run_node_extension_runtime(
    extension: &Extension,
    entry: &Path,
    mode: &str,
    command_name: Option<&str>,
    command_args: Option<&str>,
    event: Option<&str>,
    tool_name: Option<&str>,
    shortcut_name: Option<&str>,
    payload: Value,
) -> Result<Value> {
    let payload_json = serde_json::to_string(&payload)?;
    let script = r#"
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";
import { EventEmitter } from "node:events";

const entry = process.env.BBARIT_EXTENSION_ENTRY;
const mode = process.env.BBARIT_EXTENSION_MODE;
const eventName = process.env.BBARIT_EXTENSION_EVENT;
const commandName = process.env.BBARIT_EXTENSION_COMMAND;
const commandArgs = process.env.BBARIT_EXTENSION_COMMAND_ARGS || "";
const toolName = process.env.BBARIT_EXTENSION_TOOL;
const shortcutName = process.env.BBARIT_EXTENSION_SHORTCUT;
const payload = JSON.parse(process.env.BBARIT_EXTENSION_PAYLOAD || "null");
const ctx = JSON.parse(process.env.BBARIT_EXTENSION_CONTEXT || "null") || {};
const outputs = [];
const handlers = new Map();
const commands = new Map();
const tools = new Map();
const shortcuts = new Map();
const providers = new Map();
const flags = new Map();
const messageRenderers = new Map();
const extEvents = new EventEmitter();
const unregisteredProviders = new Set();
const originalConsoleLog = console.log.bind(console);
let editorText = "";
let toolsExpanded = false;

function serialize(value) {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function normalizeToolResult(result) {
  if (result === undefined || result === null) return "";
  if (typeof result === "string") return result;
  if (Array.isArray(result.content)) {
    return result.content.map((item) => {
      if (typeof item === "string") return item;
      if (item && typeof item.text === "string") return item.text;
      return serialize(item);
    }).join("\n");
  }
  if (typeof result.text === "string") return result.text;
  if (typeof result.message === "string") return result.message;
  return serialize(result);
}

function normalizeAssistantResult(result) {
  if (result === undefined || result === null) return { text: "" };
  if (typeof result === "string") return { text: result };
  if (Array.isArray(result)) return normalizeAssistantResult({ content: result });
  if (typeof result !== "object") return { text: serialize(result) };

  let text = "";
  if (typeof result.text === "string") {
    text = result.text;
  } else if (typeof result.content === "string") {
    text = result.content;
  } else if (Array.isArray(result.content)) {
    text = result.content.map((item) => {
      if (typeof item === "string") return item;
      if (item && typeof item.text === "string") return item.text;
      if (item && typeof item.content === "string") return item.content;
      return "";
    }).join("");
  } else if (typeof result.message === "string") {
    text = result.message;
  }

  const toolCalls = result.toolCalls || result.tool_calls || [];
  return {
    text,
    toolCalls: Array.isArray(toolCalls) ? toolCalls : [],
    usage: result.usage || null,
    raw: result,
  };
}

async function normalizeProviderStreamResult(result) {
  if (result && typeof result.result === "function") {
    return normalizeAssistantResult(await result.result());
  }
  if (result && typeof result[Symbol.asyncIterator] === "function") {
    let text = "";
    const toolCalls = [];
    let usage = null;
    for await (const event of result) {
      if (!event) continue;
      if (typeof event === "string") {
        text += event;
      } else if (typeof event.delta === "string") {
        text += event.delta;
      } else if (typeof event.text === "string" && String(event.type || "").includes("delta")) {
        text += event.text;
      } else if (event.type === "tool_call" || event.type === "toolCall") {
        toolCalls.push(event.toolCall || event.tool_call || event);
      } else if (event.type === "usage") {
        usage = event.usage || event;
      }
    }
    return { text, toolCalls, usage };
  }
  return normalizeAssistantResult(result);
}

function stripTypeScriptFallback(source) {
  return source
    .replace(/^\s*import\s+type\s+[^;]+;?\s*$/gm, "")
    .replace(/^\s*export\s+type\s+[^;]+;?\s*$/gm, "")
    .replace(/^\s*type\s+\w+[^;]*;?\s*$/gm, "")
    .replace(/^\s*interface\s+\w+[^{]*\{[\s\S]*?\n\}\s*$/gm, "")
    .replace(/\s+as\s+const\b/g, "")
    .replace(/\s+as\s+[A-Za-z_$][A-Za-z0-9_$<>\s|&.[\]{}'"?:-]*/g, "")
    .replace(/([({,]\s*[A-Za-z_$][A-Za-z0-9_$]*)\s*:\s*[A-Za-z_$][A-Za-z0-9_$<>\s|&.[\]{}'"?:-]*(?=\s*[,)=])/g, "$1");
}

async function transpileTypeScript(entry) {
  const source = fs.readFileSync(entry, "utf8");
  const requireFromEntry = createRequire(pathToFileURL(entry).href);
  try {
    const ts = requireFromEntry("typescript");
    return ts.transpileModule(source, {
      compilerOptions: {
        module: ts.ModuleKind.ESNext,
        target: ts.ScriptTarget.ES2022,
        jsx: ts.JsxEmit.React,
        esModuleInterop: true,
      },
    }).outputText;
  } catch {
    return stripTypeScriptFallback(source);
  }
}

async function loadExtensionModule(entry) {
  const extension = path.extname(entry).toLowerCase();
  if (![".ts", ".tsx"].includes(extension)) {
    return await import(pathToFileURL(entry).href);
  }
  const requireFromEntry = createRequire(pathToFileURL(entry).href);
  try {
    const jitiModule = requireFromEntry("jiti");
    const createJiti = jitiModule.createJiti || jitiModule.default || jitiModule;
    const jiti = createJiti(entry);
    if (typeof jiti.import === "function") {
      return await jiti.import(entry);
    }
    return await jiti(entry);
  } catch {
    const js = await transpileTypeScript(entry);
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "bbarit-extension-"));
    const tempEntry = path.join(tempDir, path.basename(entry).replace(/\.[cm]?tsx?$/, ".mjs"));
    fs.writeFileSync(tempEntry, js, "utf8");
    return await import(pathToFileURL(tempEntry).href);
  }
}

console.log = (...args) => {
  outputs.push({ type: "console", message: args.map(serialize).join(" ") });
};

function pushUi(method, data = {}) {
  outputs.push({ type: "ui", method, ...data });
}

function normalizeLines(value) {
  if (value === undefined) return null;
  if (Array.isArray(value)) return value.map(serialize);
  return serialize(value);
}

function createUiContext() {
  const theme = {
    fg(_key, text) { return String(text); },
    bg(_key, text) { return String(text); },
    bold(text) { return String(text); },
    dim(text) { return String(text); },
    italic(text) { return String(text); },
    underline(text) { return String(text); },
  };
  return {
    theme,
    notify(message, type = "info") {
      outputs.push({ type: "notify", level: String(type), message: String(message) });
    },
    setStatus(key, text) {
      pushUi("setStatus", { key: String(key), text: text === undefined ? null : String(text) });
    },
    setWidget(key, content, options = {}) {
      pushUi("setWidget", {
        key: String(key),
        content: normalizeLines(content),
        placement: options && options.placement ? String(options.placement) : "aboveEditor",
      });
    },
    setFooter(content) {
      pushUi("setFooter", { content: content === undefined ? null : "[custom-footer]" });
    },
    setHeader(content) {
      pushUi("setHeader", { content: content === undefined ? null : "[custom-header]" });
    },
    setWorkingMessage(message) {
      pushUi("setWorkingMessage", { message: message === undefined ? null : String(message) });
    },
    setWorkingVisible(visible) {
      pushUi("setWorkingVisible", { visible: Boolean(visible) });
    },
    setWorkingIndicator(indicator) {
      pushUi("setWorkingIndicator", { indicator: indicator === undefined ? null : indicator });
    },
    setTitle(title) {
      pushUi("setTitle", { title: String(title) });
    },
    setEditorText(text) {
      editorText = String(text ?? "");
      pushUi("setEditorText", { text: editorText });
    },
    getEditorText() {
      return editorText;
    },
    pasteToEditor(text) {
      const pasted = String(text ?? "");
      editorText += pasted;
      pushUi("pasteToEditor", { text: pasted });
    },
    getToolsExpanded() {
      return toolsExpanded;
    },
    setToolsExpanded(value) {
      toolsExpanded = Boolean(value);
      pushUi("setToolsExpanded", { expanded: toolsExpanded });
    },
    getAllThemes() {
      return [];
    },
    getTheme() {
      return undefined;
    },
    setTheme(themeName) {
      pushUi("setTheme", { theme: serialize(themeName) });
      return { success: false, error: "theme switching is not available in this CUI runtime" };
    },
    addAutocompleteProvider() {
      pushUi("addAutocompleteProvider", { registered: true });
      return () => {};
    },
    setEditorComponent(component) {
      pushUi("setEditorComponent", { active: component !== undefined });
    },
    getEditorComponent() {
      return undefined;
    },
    async confirm(title, message, options = {}) {
      pushUi("confirm", {
        title: String(title),
        message: String(message ?? ""),
        timeout: options && options.timeout ? Number(options.timeout) : undefined,
        result: false,
      });
      return false;
    },
    async select(title, options = [], opts = {}) {
      pushUi("select", {
        title: String(title),
        options: Array.isArray(options) ? options.map(serialize) : [],
        timeout: opts && opts.timeout ? Number(opts.timeout) : undefined,
        result: null,
      });
      return null;
    },
    async input(title, placeholder = "", options = {}) {
      pushUi("input", {
        title: String(title),
        placeholder: String(placeholder ?? ""),
        timeout: options && options.timeout ? Number(options.timeout) : undefined,
        result: null,
      });
      return null;
    },
    async editor(title, prefill = "", options = {}) {
      pushUi("editor", {
        title: String(title),
        prefill: String(prefill ?? ""),
        timeout: options && options.timeout ? Number(options.timeout) : undefined,
        result: String(prefill ?? ""),
      });
      return String(prefill ?? "");
    },
    async custom(_factory, options = {}) {
      pushUi("custom", {
        placement: options && options.placement ? String(options.placement) : undefined,
        result: null,
      });
      return null;
    },
  };
}

function createExtensionContext() {
  return {
    cwd: ctx.cwd || process.cwd(),
    mode: ctx.mode || process.env.BBARIT_EXTENSION_MODE || "interactive",
    extensionId: process.env.BBARIT_EXTENSION_ID,
    extensionRoot: process.env.BBARIT_EXTENSION_ROOT,
    hasUI: true,
    signal: undefined,
    model: ctx.model ?? null,
    notify(message) {
      outputs.push({ type: "notify", message: String(message) });
    },
    log(...args) {
      outputs.push({ type: "log", message: args.map(serialize).join(" ") });
    },
    isProjectTrusted() { return Boolean(ctx.projectTrusted); },
    getContextUsage() { return ctx.contextUsage ?? null; },
    getSystemPrompt() { return ctx.systemPrompt ?? ""; },
    getSystemPromptOptions() { return ctx.systemPromptOptions ?? {}; },
    isIdle() { return true; },
    hasPendingMessages() { return false; },
    abort() {},
    shutdown() { outputs.push({ type: "result", value: { action: "shutdown" } }); },
    async waitForIdle() {},
    async compact(options = {}) { outputs.push({ type: "result", value: { action: "compact", options } }); },
    async newSession(options = {}) { outputs.push({ type: "result", value: { action: "newSession", options } }); },
    async fork(entryId, options = {}) { outputs.push({ type: "result", value: { action: "fork", entryId: String(entryId), options } }); },
    async navigateTree(targetId, options = {}) { outputs.push({ type: "result", value: { action: "navigateTree", targetId: String(targetId), options } }); },
    async switchSession(sessionPath, options = {}) { outputs.push({ type: "result", value: { action: "switchSession", sessionPath: String(sessionPath), options } }); },
    async reload() { outputs.push({ type: "result", value: { action: "reload" } }); },
    ui: createUiContext(),
  };
}

const pi = {
  on(event, handler) {
    if (!handlers.has(event)) handlers.set(event, []);
    handlers.get(event).push(handler);
  },
  registerCommand(name, options = {}) {
    commands.set(name, options);
  },
  registerTool(tool = {}) {
    if (tool && tool.name) {
      tools.set(tool.name, tool);
    }
  },
  registerShortcut(shortcut, options = {}) {
    if (shortcut) {
      shortcuts.set(shortcut, options);
    }
  },
  registerProvider(name, config = {}) {
    if (name) {
      providers.set(name, config || {});
      unregisteredProviders.delete(name);
    }
  },
  unregisterProvider(name) {
    if (name) {
      providers.delete(name);
      unregisteredProviders.add(name);
    }
  },
  notify(message) {
    outputs.push({ type: "notify", message: String(message) });
  },
  log(...args) {
    outputs.push({ type: "log", message: args.map((arg) => typeof arg === "string" ? arg : JSON.stringify(arg)).join(" ") });
  },
  // Read-bearing context getters (live snapshot from the agent).
  getModel() { return ctx.model ?? null; },
  getModelProvider() { return ctx.provider ?? null; },
  getProvider() { return ctx.provider ?? null; },
  getCwd() { return ctx.cwd ?? process.cwd(); },
  isProjectTrusted() { return Boolean(ctx.projectTrusted); },
  getSessionName() { return ctx.sessionName ?? null; },
  getSessionId() { return ctx.sessionId ?? null; },
  getThinkingLevel() { return ctx.thinkingLevel ?? null; },
  getContextUsage() { return ctx.contextUsage ?? null; },
  getSystemPrompt() { return ctx.systemPrompt ?? ""; },
  getSystemPromptOptions() { return ctx.systemPromptOptions ?? {}; },
  getActiveTools() { return ctx.activeTools ?? []; },
  getAllTools() { return ctx.activeTools ?? []; },
  getCommands() { return [...commands.keys()]; },
  getFlag() { return undefined; },
  // Mutating actions: applied by the agent after the input hook returns.
  setModel(model) { outputs.push({ type: "result", value: { action: "setModel", model: String(model) } }); },
  setSessionName(name) { outputs.push({ type: "result", value: { action: "setSessionName", name: String(name) } }); },
  setThinkingLevel(level) { outputs.push({ type: "result", value: { action: "setThinkingLevel", level: String(level) } }); },
  sendUserMessage(content) { outputs.push({ type: "result", value: { action: "sendUserMessage", text: typeof content === "string" ? content : serialize(content) } }); },
  sendMessage(message) { outputs.push({ type: "result", value: { action: "sendUserMessage", text: typeof message === "string" ? message : serialize(message) } }); },
  appendEntry(customType, data = null) { outputs.push({ type: "result", value: { action: "appendEntry", customType: String(customType), data } }); },
  setLabel(entryId, label) { outputs.push({ type: "result", value: { action: "setLabel", entryId: String(entryId), label: String(label) } }); },
  setActiveTools(names) { outputs.push({ type: "result", value: { action: "setActiveTools", names: Array.isArray(names) ? names.map(String) : [] } }); },
  registerFlag(name, options = {}) { if (name) flags.set(String(name), options || {}); },
  registerMessageRenderer(customType, renderer) { if (customType) messageRenderers.set(String(customType), typeof renderer === "function"); },
  // Synchronous command execution available to extensions.
  exec(command, args = [], options = {}) {
    try {
      const stdout = execFileSync(String(command), Array.isArray(args) ? args.map(String) : [], {
        cwd: (options && options.cwd) || ctx.cwd || process.cwd(),
        encoding: "utf8",
        timeout: (options && options.timeout) || 30000,
        maxBuffer: 16 * 1024 * 1024,
      });
      return { ok: true, stdout: String(stdout), stderr: "", code: 0 };
    } catch (error) {
      return {
        ok: false,
        stdout: error && error.stdout ? String(error.stdout) : "",
        stderr: error && error.stderr ? String(error.stderr) : String((error && error.message) || error),
        code: error && error.status != null ? error.status : 1,
      };
    }
  },
  events: extEvents,
};

try {
  const mod = await loadExtensionModule(entry);
  const factory = mod.default || mod.extension || mod.activate;
  if (typeof factory === "function") {
    await factory(pi);
  }

  if (mode === "list_commands") {
    originalConsoleLog(JSON.stringify({
      ok: true,
      available: true,
      commands: [...commands.entries()].map(([name, options]) => ({
        name,
        description: options && options.description ? String(options.description) : "",
      })),
      outputs,
    }));
  } else if (mode === "list_shortcuts") {
    originalConsoleLog(JSON.stringify({
      ok: true,
      available: true,
      shortcuts: [...shortcuts.entries()].map(([shortcut, options]) => ({
        shortcut,
        description: options && options.description ? String(options.description) : "",
      })),
      outputs,
    }));
  } else if (mode === "list_providers") {
    originalConsoleLog(JSON.stringify({
      ok: true,
      available: true,
      providers: [...providers.entries()].map(([id, config]) => ({
        id,
        config: { ...config, streamSimple: typeof config.streamSimple === "function" },
      })),
      unregisteredProviders: [...unregisteredProviders],
      outputs,
    }));
  } else if (mode === "list_tools") {
    originalConsoleLog(JSON.stringify({
      ok: true,
      available: true,
      tools: [...tools.entries()].map(([name, tool]) => ({
        name,
        description: tool && tool.description ? String(tool.description) : "",
        parameters: tool && tool.parameters ? tool.parameters : { type: "object", properties: {} },
        promptSnippet: tool && tool.promptSnippet ? String(tool.promptSnippet) : "",
        promptGuidelines: Array.isArray(tool && tool.promptGuidelines)
          ? tool.promptGuidelines.map((item) => String(item))
          : (tool && tool.promptGuidelines ? String(tool.promptGuidelines) : ""),
      })),
      outputs,
    }));
  } else if (mode === "command") {
    const command = commands.get(commandName);
    if (!command || typeof command.handler !== "function") {
      throw new Error(`extension command not found or has no handler: ${commandName}`);
    }
    const ctx = createExtensionContext();
    const result = await command.handler(commandArgs, ctx);
    if (result !== undefined) {
      outputs.push({ type: "result", value: result });
    }
    originalConsoleLog(JSON.stringify({ ok: true, available: true, outputs }));
  } else if (mode === "tool") {
    const tool = tools.get(toolName);
    if (!tool || typeof tool.execute !== "function") {
      throw new Error(`extension tool not found or has no execute(): ${toolName}`);
    }
    const rawArgs = payload && payload.arguments ? payload.arguments : {};
    const params = typeof tool.prepareArguments === "function" ? await tool.prepareArguments(rawArgs) : rawArgs;
    const toolCallId = payload && payload.toolCallId ? String(payload.toolCallId) : toolName;
    const ctx = createExtensionContext();
    const result = await tool.execute(toolCallId, params, undefined, undefined, ctx);
    outputs.push({ type: "result", value: normalizeToolResult(result), raw: result });
    originalConsoleLog(JSON.stringify({ ok: true, available: true, outputs }));
  } else if (mode === "provider_stream_simple") {
    const providerName = payload && payload.providerName ? String(payload.providerName) : "";
    const provider = providers.get(providerName);
    if (!provider || typeof provider.streamSimple !== "function") {
      throw new Error(`extension provider not found or has no streamSimple(): ${providerName}`);
    }
    const result = await provider.streamSimple(payload.model || {}, payload.context || {}, payload.options || {});
    outputs.push({ type: "result", value: await normalizeProviderStreamResult(result) });
    originalConsoleLog(JSON.stringify({ ok: true, available: true, outputs }));
  } else if (mode === "shortcut") {
    const shortcut = shortcuts.get(shortcutName);
    if (!shortcut || typeof shortcut.handler !== "function") {
      throw new Error(`extension shortcut not found or has no handler: ${shortcutName}`);
    }
    const ctx = createExtensionContext();
    const result = await shortcut.handler(ctx);
    if (result !== undefined) {
      outputs.push({ type: "result", value: result });
    }
    originalConsoleLog(JSON.stringify({ ok: true, available: true, outputs }));
  } else {
    const hookCtx = createExtensionContext();
    for (const handler of handlers.get(eventName) || []) {
      const result = await handler(payload, hookCtx);
      if (result !== undefined) {
        outputs.push({ type: "result", value: result });
      }
    }
    originalConsoleLog(JSON.stringify({ ok: true, available: true, outputs, payload }));
  }
} catch (error) {
  originalConsoleLog(JSON.stringify({
    ok: false,
    available: true,
    error: error && error.stack ? error.stack : String(error),
    outputs,
  }));
  process.exitCode = 1;
}
"#;
    let mut child = match crate::spawn::node_command()
        .arg("--input-type=module")
        .arg("-e")
        .arg(script)
        .env("BBARIT_EXTENSION_ID", &extension.id)
        .env("BBARIT_EXTENSION_ROOT", &extension.root)
        .env("BBARIT_EXTENSION_ENTRY", entry)
        .env("BBARIT_EXTENSION_MODE", mode)
        .env("BBARIT_EXTENSION_EVENT", event.unwrap_or_default())
        .env("BBARIT_EXTENSION_COMMAND", command_name.unwrap_or_default())
        .env("BBARIT_EXTENSION_TOOL", tool_name.unwrap_or_default())
        .env(
            "BBARIT_EXTENSION_SHORTCUT",
            shortcut_name.unwrap_or_default(),
        )
        .env(
            "BBARIT_EXTENSION_COMMAND_ARGS",
            command_args.unwrap_or_default(),
        )
        .env("BBARIT_EXTENSION_PAYLOAD", payload_json)
        .env("BBARIT_EXTENSION_CONTEXT", extension_context())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({
                "ok": false,
                "available": false,
                "extensionId": extension.id,
                "mode": mode,
                "entry": entry.display().to_string(),
                "error": "node was not found on PATH"
            }));
        }
        Err(error) => return Err(error).context("failed to start node extension runtime"),
    };

    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(json!({
                "ok": false,
                "available": true,
                "extensionId": extension.id,
                "mode": mode,
                "entry": entry.display().to_string(),
                "error": "extension hook timed out after 5 seconds"
            }));
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok());
    let mut value = parsed.unwrap_or_else(|| {
        json!({
            "ok": output.status.success(),
            "available": true,
            "outputs": [],
        })
    });
    if let Some(object) = value.as_object_mut() {
        object.insert("extensionId".to_string(), json!(extension.id));
        object.insert("mode".to_string(), json!(mode));
        if let Some(event) = event {
            object.insert("event".to_string(), json!(event));
        }
        if let Some(command_name) = command_name {
            object.insert("command".to_string(), json!(command_name));
        }
        object.insert("entry".to_string(), json!(entry.display().to_string()));
        if !stderr.trim().is_empty() {
            object.insert("stderr".to_string(), json!(stderr.trim()));
        }
    }
    Ok(value)
}

fn runtime_outputs_to_text(value: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(outputs) = value.get("outputs").and_then(Value::as_array) {
        for output in outputs {
            let output_type = output
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("output");
            match output_type {
                "notify" | "log" | "console" => {
                    if let Some(message) = output.get("message").and_then(Value::as_str) {
                        lines.push(message.to_string());
                    }
                }
                "result" => {
                    if let Some(result) = output.get("value") {
                        if let Some(text) = result.as_str() {
                            lines.push(text.to_string());
                        } else {
                            lines.push(result.to_string());
                        }
                    }
                }
                "ui" => {
                    let method = output.get("method").and_then(Value::as_str).unwrap_or("ui");
                    let detail = match method {
                        "setStatus" => {
                            let key = output.get("key").and_then(Value::as_str).unwrap_or("");
                            let text = output
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or("<clear>");
                            format!("ui.setStatus {key}: {text}")
                        }
                        "setWidget" => {
                            let key = output.get("key").and_then(Value::as_str).unwrap_or("");
                            let placement = output
                                .get("placement")
                                .and_then(Value::as_str)
                                .unwrap_or("aboveEditor");
                            let content = output
                                .get("content")
                                .map(|value| {
                                    if let Some(lines) = value.as_array() {
                                        lines
                                            .iter()
                                            .filter_map(Value::as_str)
                                            .collect::<Vec<_>>()
                                            .join(" | ")
                                    } else if let Some(text) = value.as_str() {
                                        text.to_string()
                                    } else {
                                        value.to_string()
                                    }
                                })
                                .unwrap_or_else(|| "<clear>".to_string());
                            format!("ui.setWidget {key} ({placement}): {content}")
                        }
                        "setTitle" => format!(
                            "ui.setTitle {}",
                            output.get("title").and_then(Value::as_str).unwrap_or("")
                        ),
                        "setEditorText" => format!(
                            "ui.setEditorText {}",
                            output.get("text").and_then(Value::as_str).unwrap_or("")
                        ),
                        "pasteToEditor" => format!(
                            "ui.pasteToEditor {}",
                            output.get("text").and_then(Value::as_str).unwrap_or("")
                        ),
                        "setWorkingMessage" => format!(
                            "ui.setWorkingMessage {}",
                            output
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("<default>")
                        ),
                        "setWorkingVisible" => format!(
                            "ui.setWorkingVisible {}",
                            output
                                .get("visible")
                                .and_then(Value::as_bool)
                                .unwrap_or(false)
                        ),
                        "setWorkingIndicator" => "ui.setWorkingIndicator".to_string(),
                        "setToolsExpanded" => format!(
                            "ui.setToolsExpanded {}",
                            output
                                .get("expanded")
                                .and_then(Value::as_bool)
                                .unwrap_or(false)
                        ),
                        "confirm" | "select" | "input" | "editor" | "custom" => {
                            let title = output.get("title").and_then(Value::as_str).unwrap_or("");
                            format!("ui.{method} {title}")
                        }
                        other => format!("ui.{other} {}", output),
                    };
                    lines.push(detail);
                }
                _ => {}
            }
        }
    }
    if lines.is_empty() {
        "Extension command completed.".to_string()
    } else {
        lines.join("\n")
    }
}

fn extension_base_dirs(config: &AppConfig) -> Vec<PathBuf> {
    let mut dirs = vec![config.user_app_dir.join("extensions")];
    if config.project_trusted {
        dirs.push(config.app_dir.join("extensions"));
    }
    dirs
}

fn collect_extension_path(path: &Path, out: &mut BTreeMap<String, Extension>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        let root = path.parent().unwrap_or_else(|| Path::new("."));
        if is_extension_source_file(path) {
            let extension = extension_from_entry(root, path)?;
            out.insert(extension.id.clone(), extension);
        } else if let Some(extension) = read_extension_manifest(root, path)? {
            out.insert(extension.id.clone(), extension);
        }
        return Ok(());
    }
    try_load_extension(path, out)?;
    Ok(())
}

fn collect_extensions_from_base(base: &Path, out: &mut BTreeMap<String, Extension>) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }
    try_load_extension(base, out)?;
    for entry in fs::read_dir(base).with_context(|| format!("failed to read {}", base.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            try_load_extension(&path, out)?;
        }
    }
    Ok(())
}

fn collect_package_extensions(
    package: &PackageSpec,
    out: &mut BTreeMap<String, Extension>,
) -> Result<()> {
    let root = package.resolved_root();
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        collect_extension_path(&root, out)?;
        return Ok(());
    }
    if let Some(filter) = package.extensions.as_ref() {
        for entry in filter {
            collect_package_extension_entry(&root.join(entry), out)?;
        }
        return Ok(());
    }
    if let Some(entries) = package_extension_manifest_entries(&root)? {
        for entry in entries {
            collect_package_extension_entry(&root.join(entry), out)?;
        }
    } else {
        collect_package_extension_entry(&root.join("extensions"), out)?;
    }
    Ok(())
}

fn collect_package_extension_entry(
    path: &Path,
    out: &mut BTreeMap<String, Extension>,
) -> Result<()> {
    if path.is_dir() {
        collect_extensions_from_base(path, out)
    } else {
        collect_extension_path(path, out)
    }
}

fn package_extension_manifest_entries(root: &Path) -> Result<Option<Vec<String>>> {
    let path = root.join("package.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read package manifest {}", path.display()))?;
    let package: PackageJson = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(package.pi.and_then(|manifest| manifest.extensions))
}

fn try_load_extension(root: &Path, out: &mut BTreeMap<String, Extension>) -> Result<()> {
    let mut loaded = false;
    for manifest_name in ["extension.json", "pi-extension.json", "package.json"] {
        let path = root.join(manifest_name);
        if !path.exists() {
            continue;
        }
        let Some(extension) = read_extension_manifest(root, &path)? else {
            continue;
        };
        out.insert(extension.id.clone(), extension);
        loaded = true;
        break;
    }
    if !loaded && let Some(entry) = resolve_extension_entry(root, None) {
        let extension = extension_from_entry(root, &entry)?;
        out.insert(extension.id.clone(), extension);
    }
    Ok(())
}

fn read_extension_manifest(root: &Path, path: &Path) -> Result<Option<Extension>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read extension manifest {}", path.display()))?;
    let raw = raw.trim_start_matches('\u{feff}');
    let manifest = if path.file_name().and_then(|name| name.to_str()) == Some("package.json") {
        let package: PackageJson = serde_json::from_str(raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        let Some(mut manifest) = package.pi_agent else {
            return Ok(None);
        };
        if manifest.entry.is_none() {
            manifest.entry = package.main.or(package.module);
        }
        if manifest.name.is_none() {
            manifest.name = package.name;
        }
        if manifest.description.is_none() {
            manifest.description = package.description;
        }
        manifest
    } else {
        serde_json::from_str(raw).with_context(|| format!("failed to parse {}", path.display()))?
    };
    let fallback_id = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("extension")
        .to_string();
    let id = manifest.id.unwrap_or_else(|| fallback_id.clone());
    let name = manifest.name.unwrap_or_else(|| id.clone());
    let description = manifest.description.unwrap_or_default();
    let entry = resolve_extension_entry(root, manifest.entry.or(manifest.main));
    let commands = normalize_commands(&id, root, manifest.commands);
    let hooks = discover_extension_hooks(root, manifest.hooks)?;
    Ok(Some(Extension {
        id,
        name,
        description,
        root: root.to_path_buf(),
        entry,
        commands,
        hooks,
    }))
}

fn extension_from_entry(root: &Path, entry: &Path) -> Result<Extension> {
    let fallback_id = entry
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("extension")
        .to_string();
    let hooks = discover_extension_hooks(root, None)?;
    Ok(Extension {
        id: fallback_id.clone(),
        name: fallback_id,
        description: String::new(),
        root: root.to_path_buf(),
        entry: Some(entry.to_path_buf()),
        commands: Vec::new(),
        hooks,
    })
}

fn resolve_extension_entry(root: &Path, hint: Option<String>) -> Option<PathBuf> {
    if let Some(hint) = hint {
        let entry = root.join(hint);
        if entry.is_file() {
            return Some(entry);
        }
    }
    [
        "index.mjs",
        "index.js",
        "index.cjs",
        "index.ts",
        "index.tsx",
        "index.jsx",
        "src/index.mjs",
        "src/index.js",
        "src/index.cjs",
        "src/index.ts",
        "src/index.tsx",
        "src/index.jsx",
    ]
    .into_iter()
    .map(|candidate| root.join(candidate))
    .find(|candidate| candidate.is_file())
}

const EXTENSION_EVENT_NAMES: &[&str] = &[
    "project_trust",
    "resources_discover",
    "session_start",
    "session_before_switch",
    "session_before_fork",
    "session_before_compact",
    "session_compact",
    "session_shutdown",
    "session_before_tree",
    "session_tree",
    "context",
    "before_provider_request",
    "after_provider_response",
    "before_agent_start",
    "agent_start",
    "agent_end",
    "turn_start",
    "turn_end",
    "message_start",
    "message_update",
    "message_end",
    "tool_execution_start",
    "tool_execution_update",
    "tool_execution_end",
    "model_select",
    "thinking_level_select",
    "tool_call",
    "tool_result",
    "user_bash",
    "input",
];

fn discover_extension_hooks(
    root: &Path,
    manifest_hooks: Option<Vec<String>>,
) -> Result<Vec<String>> {
    let mut hooks = BTreeSet::new();
    for hook in manifest_hooks.into_iter().flatten() {
        if EXTENSION_EVENT_NAMES.contains(&hook.as_str()) {
            hooks.insert(hook);
        }
    }
    discover_extension_hooks_in_dir(root, 0, &mut hooks)?;
    Ok(hooks.into_iter().collect())
}

fn discover_extension_hooks_in_dir(
    dir: &Path,
    depth: usize,
    hooks: &mut BTreeSet<String>,
) -> Result<()> {
    if depth > 4 || !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if matches!(name, "node_modules" | ".git" | "target" | "dist" | "build") {
                continue;
            }
            discover_extension_hooks_in_dir(&path, depth + 1, hooks)?;
            continue;
        }
        if !is_extension_source_file(&path) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        collect_hook_names_from_source(&source, hooks);
    }
    Ok(())
}

fn is_extension_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs")
    )
}

fn collect_hook_names_from_source(source: &str, hooks: &mut BTreeSet<String>) {
    for event in EXTENSION_EVENT_NAMES {
        for quote in ['"', '\'', '`'] {
            let target = format!(".on({quote}{event}{quote}");
            let target_with_space = format!(".on({quote}{event}{quote},");
            let target_on = format!("on({quote}{event}{quote}");
            if source.contains(&target)
                || source.contains(&target_with_space)
                || source.contains(&target_on)
            {
                hooks.insert((*event).to_string());
            }
        }
    }
}

fn normalize_commands(
    extension_id: &str,
    root: &Path,
    commands: Option<CommandsManifest>,
) -> Vec<ExtensionCommand> {
    let Some(commands) = commands else {
        return Vec::new();
    };
    let entries = match commands {
        CommandsManifest::List(list) => list
            .into_iter()
            .map(|command| (command.name.clone().unwrap_or_default(), command))
            .collect::<Vec<_>>(),
        CommandsManifest::Map(map) => map.into_iter().collect::<Vec<_>>(),
    };
    entries
        .into_iter()
        .filter_map(|(key, command)| {
            let name = command
                .name
                .or(if key.is_empty() { None } else { Some(key) })?;
            Some(ExtensionCommand {
                extension_id: extension_id.to_string(),
                name,
                description: command.description.unwrap_or_default(),
                root: root.to_path_buf(),
                prompt: command.prompt,
                prompt_path: command.prompt_path.map(PathBuf::from),
                shell: command.command,
                runtime: false,
            })
        })
        .collect()
}

fn parse_command_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in args.chars() {
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn substitute_args(content: &str, args: &[String]) -> String {
    substitute_args_with(content, args, false)
}

// Shell templates run through bash, so user-supplied argument values are
// single-quoted to keep `; rm -rf ~` style input inert. Template text itself
// is left untouched.
fn substitute_shell_args(content: &str, args: &[String]) -> String {
    substitute_args_with(content, args, true)
}

// Single pass over the original template so `$10` is not corrupted by a `$1`
// replacement and substituted values are never re-scanned for placeholders.
fn substitute_args_with(content: &str, args: &[String], quote: bool) -> String {
    let render = |value: &str| {
        if quote {
            shell_quote(value)
        } else {
            value.to_string()
        }
    };
    let all_args = args
        .iter()
        .map(|arg| render(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let mut output = String::new();
    let mut index = 0;
    while index < content.len() {
        let Some(ch) = content[index..].chars().next() else {
            break;
        };
        if ch != '$' {
            output.push(ch);
            index += ch.len_utf8();
            continue;
        }

        let after_dollar = index + 1;
        let rest = &content[after_dollar..];
        if let Some(rest) = rest.strip_prefix("ARGUMENTS") {
            output.push_str(&all_args);
            index = content.len() - rest.len();
            continue;
        }
        if let Some(rest) = rest.strip_prefix('@') {
            output.push_str(&all_args);
            index = content.len() - rest.len();
            continue;
        }

        let digit_len = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .map(char::len_utf8)
            .sum::<usize>();
        if digit_len > 0 {
            let value = rest[..digit_len]
                .parse::<usize>()
                .ok()
                .and_then(|position| args.get(position.saturating_sub(1)))
                .cloned()
                .unwrap_or_default();
            output.push_str(&render(&value));
            index = after_dollar + digit_len;
            continue;
        }

        output.push('$');
        index = after_dollar;
    }
    output
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_substitution_quotes_injected_args() {
        let args = vec!["x; touch /tmp/pwn".to_string()];
        assert_eq!(
            substitute_shell_args("echo $1", &args),
            "echo 'x; touch /tmp/pwn'"
        );
    }

    #[test]
    fn shell_substitution_escapes_embedded_single_quotes() {
        let args = vec!["a'b".to_string()];
        assert_eq!(substitute_shell_args("echo $1", &args), "echo 'a'\\''b'");
    }

    #[test]
    fn shell_substitution_quotes_each_argument_of_all_args() {
        let args = vec!["one; ls".to_string(), "two".to_string()];
        assert_eq!(
            substitute_shell_args("run $ARGUMENTS", &args),
            "run 'one; ls' 'two'"
        );
    }

    #[test]
    fn substitute_args_handles_two_digit_placeholders() {
        let args: Vec<String> = (1..=10).map(|i| format!("a{i}")).collect();
        assert_eq!(substitute_args("$10 $1", &args), "a10 a1");
    }

    #[test]
    fn substitute_args_does_not_rescan_substituted_values() {
        let args = vec!["$2".to_string(), "second".to_string()];
        assert_eq!(substitute_args("$1 $2", &args), "$2 second");
    }

    #[test]
    fn substitute_args_expands_arguments_and_at() {
        let args = vec!["one".to_string(), "two".to_string()];
        assert_eq!(substitute_args("$ARGUMENTS|$@", &args), "one two|one two");
        // Missing positions and bare dollars pass through predictably.
        assert_eq!(substitute_args("$3 costs $", &args), " costs $");
    }
}
