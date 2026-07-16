use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::config::AppConfig;
use crate::providers::{Model, ThinkingLevel};

pub const CURRENT_SESSION_VERSION: u32 = 3;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub cwd: PathBuf,
    pub name: Option<String>,
    pub current_model: Option<String>,
    pub current_node: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: Role,
    pub content: String,
    pub model: Option<String>,
    pub created_at: String,
    /// Image attachments as data URLs ("data:image/png;base64,…"); empty for
    /// text-only messages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    #[serde(default)]
    pub input: usize,
    #[serde(default)]
    pub output: usize,
    #[serde(default, rename = "cacheRead")]
    pub cache_read: usize,
    #[serde(default, rename = "cacheWrite")]
    pub cache_write: usize,
    #[serde(default)]
    pub total: usize,
}

impl TokenUsage {
    pub fn new(
        input: usize,
        output: usize,
        cache_read: usize,
        cache_write: usize,
        total: usize,
    ) -> Self {
        let computed = input + output + cache_read + cache_write;
        Self {
            input,
            output,
            cache_read,
            cache_write,
            total: if total > 0 { total } else { computed },
        }
    }

    pub fn add_assign(&mut self, other: &Self) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.total += other.total;
    }

    pub fn is_empty(&self) -> bool {
        self.input == 0
            && self.output == 0
            && self.cache_read == 0
            && self.cache_write == 0
            && self.total == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionHeader {
    #[serde(rename = "type")]
    entry_type: String,
    version: u32,
    id: String,
    timestamp: String,
    cwd: String,
    #[serde(rename = "parentSession", skip_serializing_if = "Option::is_none")]
    parent_session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentMessage {
    role: Role,
    content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
    #[serde(default, rename = "toolCalls", skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ToolCallRecord>,
    #[serde(
        default,
        rename = "toolCallId",
        skip_serializing_if = "Option::is_none"
    )]
    tool_call_id: Option<String>,
    #[serde(default, rename = "toolName", skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(
        default,
        rename = "isError",
        skip_serializing_if = "std::ops::Not::not"
    )]
    is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionEntry {
    Message {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        message: AgentMessage,
    },
    ModelChange {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        provider: String,
        #[serde(rename = "modelId")]
        model_id: String,
        #[serde(
            default,
            rename = "thinkingLevel",
            skip_serializing_if = "Option::is_none"
        )]
        thinking_level: Option<ThinkingLevel>,
    },
    SessionInfo {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        name: Option<String>,
    },
    Compaction {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        summary: String,
        #[serde(rename = "firstKeptEntryId")]
        first_kept_entry_id: String,
        #[serde(rename = "tokensBefore")]
        tokens_before: usize,
    },
    Label {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        #[serde(rename = "targetId")]
        target_id: String,
        label: Option<String>,
    },
    BranchSummary {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        summary: String,
        #[serde(rename = "fromId")]
        from_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
        #[serde(
            default,
            rename = "fromHook",
            skip_serializing_if = "std::ops::Not::not"
        )]
        from_hook: bool,
    },
    Custom {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        #[serde(rename = "customType")]
        custom_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
    },
    CustomMessage {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        timestamp: String,
        #[serde(rename = "customType")]
        custom_type: String,
        content: Value,
        #[serde(default = "default_true")]
        display: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
    },
}

fn default_true() -> bool {
    true
}

/// Build an in-context Message from a custom/branch-summary entry.
fn custom_text_message(
    id: &str,
    parent_id: &Option<String>,
    timestamp: &str,
    text: &str,
    model: Option<String>,
) -> Message {
    Message {
        id: id.to_string(),
        parent_id: parent_id.clone(),
        role: Role::User,
        content: text.to_string(),
        model,
        created_at: timestamp.to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        tool_name: None,
        is_error: false,
        usage: None,
    }
}

/// Flatten a CustomMessage content value (string or content blocks) to text.
fn content_value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| item.as_str().map(ToOwned::to_owned))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub struct SessionStore {
    session: Session,
    messages: Vec<Message>,
    entries: Vec<SessionEntry>,
    path: Option<PathBuf>,
}

impl SessionStore {
    pub fn open(config: &AppConfig, cli: &Cli) -> Result<Self> {
        if cli.no_session {
            return Self::new_memory(config, cli.session_id.clone());
        }
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        if let Some(source) = &cli.fork {
            return Self::fork_from_path(config, source);
        }
        if let Some(session_id) = &cli.session_id {
            return Self::open_or_create_session_id(config, session_id);
        }
        if let Some(session) = &cli.session {
            return Self::open_path(resolve_session_path(session, &config.session_dir)?);
        }
        if cli.resume {
            return Self::select(&config.session_dir);
        }
        if cli.continue_session {
            return Self::open_path(latest_session(&config.session_dir)?);
        }
        Self::create(config)
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn session_file(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn create_new(config: &AppConfig) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        Self::create(config)
    }

    pub fn open_or_create_session_id(config: &AppConfig, session_id: &str) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        validate_session_id(session_id)?;
        let db = config.session_dir.join(format!("{session_id}.db"));
        let jsonl = config.session_dir.join(format!("{session_id}.jsonl"));
        if db.exists() {
            Self::open_path(db)
        } else if jsonl.exists() {
            Self::open_path(jsonl)
        } else {
            Self::create_with_id(config, session_id.to_string())
        }
    }

    pub fn open_session_ref(config: &AppConfig, reference: &str) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        Self::open_path(resolve_session_path(reference, &config.session_dir)?)
    }

    pub fn open_latest(config: &AppConfig) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        Self::open_path(latest_session(&config.session_dir)?)
    }

    pub fn export_session_ref_html(
        config: &AppConfig,
        reference: &str,
        target: impl AsRef<Path>,
    ) -> Result<()> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        let store = Self::open_path(resolve_session_path(reference, &config.session_dir)?)?;
        store.export_html(target)
    }

    pub fn import_jsonl(config: &AppConfig, source: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        let source = source.as_ref();
        let source = if source.is_absolute() {
            source.to_path_buf()
        } else {
            config.cwd.join(source)
        };
        if !source.exists() {
            bail!("session import file not found: {}", source.display());
        }
        let source = fs::canonicalize(&source)
            .with_context(|| format!("failed to resolve {}", source.display()))?;
        let file_name = source
            .file_name()
            .ok_or_else(|| anyhow!("session import path has no file name"))?;
        // Validate before copying so a file that does not parse as a session
        // never lands in session_dir.
        Self::open_path(source.clone())
            .with_context(|| format!("not a valid session file: {}", source.display()))?;
        let mut destination = config.session_dir.join(file_name);
        let same_file = fs::canonicalize(&destination)
            .map(|destination| destination == source)
            .unwrap_or(false);
        if !same_file {
            // A different session may already own this basename; uniquify the
            // destination instead of silently overwriting it.
            if destination.exists() {
                let stem = destination
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("session")
                    .to_string();
                let extension = destination
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|ext| format!(".{ext}"))
                    .unwrap_or_default();
                let mut counter = 1;
                while destination.exists() {
                    destination = config
                        .session_dir
                        .join(format!("{stem}-{counter}{extension}"));
                    counter += 1;
                }
            }
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "failed to import session {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
        Self::open_path(destination)
    }

    pub fn list_session_lines(config: &AppConfig) -> Result<Vec<String>> {
        let files = session_files(&config.session_dir)?;
        let mut lines = Vec::new();
        for path in files {
            if let Ok(store) = Self::open_path(path.clone()) {
                // Prefer an explicit name; otherwise preview the first user
                // message so the session is recognizable instead of a UUID.
                let label = store
                    .session
                    .name
                    .clone()
                    .filter(|name| !name.trim().is_empty() && name != "-")
                    .unwrap_or_else(|| session_preview(&store.messages));
                lines.push(format!(
                    "{}\t{}\t{} messages\t{}\t{}",
                    store.session.id,
                    label,
                    store.messages.len(),
                    store.session.created_at,
                    path.display()
                ));
            }
        }
        Ok(lines)
    }

    pub fn fork_from_path(config: &AppConfig, source: &str) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        let source_path = PathBuf::from(source);
        let source_path = if source_path.exists() {
            source_path
        } else {
            resolve_session_path(source, &config.session_dir)?
        };
        let source_store = Self::open_path(source_path.clone())?;
        let mut fork = Self::create(config)?;
        fork.rewrite_with_entries(
            source_store.entries.clone(),
            Some(source_path.display().to_string()),
        )?;
        Ok(fork)
    }

    pub fn clone_current(&self, config: &AppConfig) -> Result<Self> {
        fs::create_dir_all(&config.session_dir)
            .with_context(|| format!("failed to create {}", config.session_dir.display()))?;
        let mut cloned = Self::create(config)?;
        cloned.rewrite_with_entries(
            self.entries.clone(),
            self.path.as_ref().map(|path| path.display().to_string()),
        )?;
        // rewrite_with_entries points the head at the last entry in file
        // order; keep the source's head (e.g. set by /branch) so cloning does
        // not jump back to an abandoned branch tip.
        if let Some(node) = &self.session.current_node
            && cloned
                .entries
                .iter()
                .any(|entry| entry_id(entry) == node.as_str())
        {
            cloned.session.current_node = Some(node.clone());
        }
        Ok(cloned)
    }

    pub fn export_html(&self, target: impl AsRef<Path>) -> Result<()> {
        let target = target.as_ref();
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let title = self
            .session
            .name
            .as_deref()
            .unwrap_or(self.session.id.as_str());
        let source = self
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "in-memory".to_string());
        let compacted = self.conversation();

        let mut html = String::new();
        html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
        html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
        html.push_str("<title>");
        html.push_str(&html_escape(title));
        html.push_str("</title><style>");
        html.push_str(EXPORT_HTML_CSS);
        html.push_str("</style></head><body>");
        html.push_str("<header><div><p class=\"eyebrow\">bbarit Session Export</p><h1>");
        html.push_str(&html_escape(title));
        html.push_str("</h1></div><dl class=\"meta\">");
        push_meta(&mut html, "Session", &self.session.id);
        push_meta(&mut html, "Created", &self.session.created_at);
        push_meta(&mut html, "Cwd", &self.session.cwd.display().to_string());
        push_meta(
            &mut html,
            "Model",
            self.session.current_model.as_deref().unwrap_or("-"),
        );
        push_meta(
            &mut html,
            "Head",
            self.session.current_node.as_deref().unwrap_or("-"),
        );
        push_meta(&mut html, "Source", &source);
        html.push_str("</dl></header>");

        html.push_str("<main><section><h2>Tree</h2><div class=\"tree\">");
        for entry in &self.entries {
            push_html_entry(&mut html, entry);
        }
        html.push_str("</div></section>");

        html.push_str("<section><h2>Compacted Conversation View</h2><div class=\"conversation\">");
        for message in &compacted {
            push_html_message(&mut html, message);
        }
        html.push_str("</div></section></main>");
        html.push_str("</body></html>");

        fs::write(target, html)
            .with_context(|| format!("failed to write HTML export {}", target.display()))?;
        Ok(())
    }

    pub fn export_jsonl(&self, target: impl AsRef<Path>) -> Result<()> {
        let target = target.as_ref();
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let header = SessionHeader {
            entry_type: "session".to_string(),
            version: CURRENT_SESSION_VERSION,
            id: self.session.id.clone(),
            timestamp: now(),
            cwd: self.session.cwd.display().to_string(),
            parent_session: None,
        };
        let mut lines = vec![serde_json::to_string(&header)?];
        let mut previous_id = None;
        for entry in self.current_branch_entries() {
            let linear = reparent_entry(entry, previous_id.clone());
            previous_id = Some(entry_id(&linear).to_string());
            lines.push(serde_json::to_string(&linear)?);
        }
        fs::write(target, format!("{}\n", lines.join("\n")))
            .with_context(|| format!("failed to write JSONL export {}", target.display()))?;
        Ok(())
    }

    pub fn set_name(&mut self, name: &str) -> Result<()> {
        self.session.name = Some(name.trim().to_string());
        let entry = SessionEntry::SessionInfo {
            id: new_entry_id(),
            parent_id: self.session.current_node.clone(),
            timestamp: now(),
            name: self.session.name.clone(),
        };
        self.append_entry(entry)
    }

    #[allow(dead_code)]
    pub fn set_model(&mut self, model: &Model) -> Result<()> {
        self.set_model_with_thinking(model, None)
    }

    pub fn set_model_with_thinking(
        &mut self,
        model: &Model,
        thinking: Option<ThinkingLevel>,
    ) -> Result<()> {
        let next = model_reference(model, thinking);
        if self.session.current_model.as_deref() == Some(next.as_str()) {
            return Ok(());
        }
        self.session.current_model = Some(next);
        let entry = SessionEntry::ModelChange {
            id: new_entry_id(),
            parent_id: self.session.current_node.clone(),
            timestamp: now(),
            provider: model.provider.clone(),
            model_id: model.id.clone(),
            thinking_level: thinking,
        };
        self.append_entry(entry)
    }

    pub fn push(
        &mut self,
        role: Role,
        content: impl Into<String>,
        model: Option<String>,
    ) -> Result<Message> {
        self.push_message(role, content, model, Vec::new(), None, None, false, None)
    }

    /// Push a user message carrying image attachments (data URLs).
    pub fn push_user_with_images(
        &mut self,
        content: impl Into<String>,
        images: Vec<String>,
    ) -> Result<Message> {
        let content = content.into();
        let id = new_entry_id();
        let parent_id = self.session.current_node.clone();
        let timestamp = now();
        let message = Message {
            id: id.clone(),
            parent_id: parent_id.clone(),
            role: Role::User,
            content: content.clone(),
            model: None,
            created_at: timestamp.clone(),
            images: images.clone(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            is_error: false,
            usage: None,
        };
        let entry = SessionEntry::Message {
            id,
            parent_id,
            timestamp,
            message: AgentMessage {
                role: Role::User,
                content,
                images,
                tool_calls: Vec::new(),
                tool_call_id: None,
                tool_name: None,
                is_error: false,
                usage: None,
            },
        };
        self.messages.push(message.clone());
        self.append_entry(entry)?;
        Ok(message)
    }

    pub fn push_assistant_with_usage(
        &mut self,
        content: impl Into<String>,
        model: Option<String>,
        usage: Option<TokenUsage>,
    ) -> Result<Message> {
        self.push_message(
            Role::Assistant,
            content,
            model,
            Vec::new(),
            None,
            None,
            false,
            usage,
        )
    }

    pub fn push_assistant_with_tool_calls(
        &mut self,
        content: impl Into<String>,
        model: Option<String>,
        tool_calls: Vec<ToolCallRecord>,
        usage: Option<TokenUsage>,
    ) -> Result<Message> {
        self.push_message(
            Role::Assistant,
            content,
            model,
            tool_calls,
            None,
            None,
            false,
            usage,
        )
    }

    pub fn push_tool_result(
        &mut self,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Result<Message> {
        self.push_message(
            Role::Tool,
            content,
            None,
            Vec::new(),
            Some(tool_call_id.into()),
            Some(tool_name.into()),
            is_error,
            None,
        )
    }

    // Internal full-fat constructor behind the thin push()/push_tool() wrappers;
    // callers never see this arity.
    #[allow(clippy::too_many_arguments)]
    fn push_message(
        &mut self,
        role: Role,
        content: impl Into<String>,
        model: Option<String>,
        tool_calls: Vec<ToolCallRecord>,
        tool_call_id: Option<String>,
        tool_name: Option<String>,
        is_error: bool,
        usage: Option<TokenUsage>,
    ) -> Result<Message> {
        let content = content.into();
        let id = new_entry_id();
        let parent_id = self.session.current_node.clone();
        let timestamp = now();
        let message = Message {
            id: id.clone(),
            parent_id: parent_id.clone(),
            role: role.clone(),
            content: content.clone(),
            model,
            created_at: timestamp.clone(),
            images: Vec::new(),
            tool_calls: tool_calls.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            is_error,
            usage: usage.clone(),
        };
        let entry = SessionEntry::Message {
            id,
            parent_id,
            timestamp,
            message: AgentMessage {
                role,
                content,
                images: Vec::new(),
                tool_calls,
                tool_call_id,
                tool_name,
                is_error,
                usage,
            },
        };
        self.messages.push(message.clone());
        self.append_entry(entry)?;
        Ok(message)
    }

    /// Entries on the active branch, root -> leaf, by walking parent links from
    /// `current_node`. This is what makes /branch and /fork scope context: only
    /// the ancestry of the current head is in play, not every entry in the file.
    fn active_path_entries(&self) -> Vec<&SessionEntry> {
        let Some(leaf) = self.session.current_node.as_deref() else {
            return self.entries.iter().collect();
        };
        let by_id: std::collections::HashMap<&str, &SessionEntry> = self
            .entries
            .iter()
            .map(|entry| (entry_id(entry), entry))
            .collect();
        let mut path = Vec::new();
        let mut cursor = Some(leaf);
        let mut guard = 0;
        while let Some(id) = cursor {
            let Some(entry) = by_id.get(id) else {
                break;
            };
            path.push(*entry);
            cursor = entry_parent_id(entry);
            guard += 1;
            if guard > self.entries.len() + 1 {
                break; // defend against a malformed parent cycle
            }
        }
        path.reverse();
        path
    }

    pub fn conversation(&self) -> Vec<Message> {
        let path = self.active_path_entries();
        let compaction_index = path
            .iter()
            .rposition(|entry| matches!(entry, SessionEntry::Compaction { .. }));

        let Some(compaction_index) = compaction_index else {
            return path
                .iter()
                .filter_map(|entry| entry_message(entry, None))
                .collect();
        };

        let SessionEntry::Compaction {
            id,
            timestamp,
            summary,
            first_kept_entry_id,
            tokens_before,
            ..
        } = path[compaction_index]
        else {
            unreachable!();
        };

        let mut compacted = vec![Message {
            id: id.clone(),
            parent_id: None,
            role: Role::User,
            content: format!(
                "Previous conversation was compacted. Summary (about {tokens_before} tokens before compaction):\n\n{summary}"
            ),
            model: None,
            created_at: timestamp.clone(),
            images: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            is_error: false,
            usage: None,
        }];

        let mut found_first_kept = false;
        for entry in &path[..compaction_index] {
            if entry_id(entry) == first_kept_entry_id {
                found_first_kept = true;
            }
            if found_first_kept && let Some(message) = entry_message(entry, None) {
                compacted.push(message);
            }
        }
        for entry in &path[compaction_index + 1..] {
            if let Some(message) = entry_message(entry, None) {
                compacted.push(message);
            }
        }
        compacted
    }

    pub fn raw_conversation(&self) -> Vec<Message> {
        self.messages.clone()
    }

    pub fn token_usage_total(&self) -> TokenUsage {
        let mut total = TokenUsage::default();
        for usage in self
            .messages
            .iter()
            .filter_map(|message| message.usage.as_ref())
        {
            total.add_assign(usage);
        }
        total
    }

    pub fn token_usage_by_model(&self) -> BTreeMap<String, TokenUsage> {
        let mut totals = BTreeMap::new();
        for message in &self.messages {
            let Some(usage) = message.usage.as_ref() else {
                continue;
            };
            let model = message.model.clone().unwrap_or_else(|| "-".to_string());
            totals
                .entry(model)
                .or_insert_with(TokenUsage::default)
                .add_assign(usage);
        }
        totals
    }

    pub fn last_token_usage(&self) -> Option<(&str, &TokenUsage)> {
        self.messages.iter().rev().find_map(|message| {
            message
                .usage
                .as_ref()
                .map(|usage| (message.model.as_deref().unwrap_or("-"), usage))
        })
    }

    pub fn append_compaction(
        &mut self,
        summary: impl Into<String>,
        keep_last_messages: usize,
    ) -> Result<String> {
        let tokens_before = self
            .messages
            .iter()
            .map(|message| estimate_tokens(&message.content))
            .sum();
        self.append_compaction_with_tokens(summary, keep_last_messages, tokens_before)
    }

    pub fn append_compaction_with_tokens(
        &mut self,
        summary: impl Into<String>,
        keep_last_messages: usize,
        tokens_before: usize,
    ) -> Result<String> {
        // keep_last_messages counts messages on the active branch, so the
        // first kept id must come from conversation(), not the flat file-order
        // list — otherwise it can land on an abandoned branch and the kept
        // messages silently drop out of context.
        let conversation = self.conversation();
        let first_kept_entry_id = conversation
            .iter()
            .rev()
            .take(keep_last_messages.max(1))
            .next_back()
            .map(|message| message.id.clone())
            .or_else(|| conversation.first().map(|message| message.id.clone()))
            .ok_or_else(|| anyhow!("no messages to compact"))?;
        let id = new_entry_id();
        let entry = SessionEntry::Compaction {
            id: id.clone(),
            parent_id: self.session.current_node.clone(),
            timestamp: now(),
            summary: summary.into(),
            first_kept_entry_id,
            tokens_before,
        };
        self.append_entry(entry)?;
        Ok(id)
    }

    pub fn new_branch_at(&mut self, node_prefix: &str) -> Result<()> {
        let entry_id = self.resolve_entry_id(node_prefix)?;
        self.session.current_node = Some(entry_id);
        Ok(())
    }

    pub fn set_label(&mut self, node_prefix: &str, label: Option<String>) -> Result<String> {
        let target_id = self.resolve_entry_id(node_prefix)?;
        let id = new_entry_id();
        let entry = SessionEntry::Label {
            id: id.clone(),
            parent_id: self.session.current_node.clone(),
            timestamp: now(),
            target_id,
            label: label.map(|value| value.replace(['\r', '\n'], " ").trim().to_string()),
        };
        self.append_entry(entry)?;
        Ok(id)
    }

    fn resolve_entry_id(&self, node_prefix: &str) -> Result<String> {
        let matches: Vec<&str> = self
            .entries
            .iter()
            .map(entry_id)
            .filter(|id| id.starts_with(node_prefix))
            .collect();
        match matches.as_slice() {
            [] => bail!("no entry starts with {node_prefix}"),
            [id] => Ok((*id).to_string()),
            // Picking the first match would silently target the oldest entry;
            // make /branch and /label fail loudly instead.
            candidates => bail!(
                "entry prefix {node_prefix} is ambiguous: matches {}",
                candidates
                    .iter()
                    .map(|id| short_id(id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub fn tree_lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| match entry {
                SessionEntry::Message {
                    id,
                    parent_id,
                    message,
                    ..
                } => format!(
                    "{} <- {}  message:{:?}  {}",
                    short_id(id),
                    parent_id
                        .as_deref()
                        .map(short_id)
                        .unwrap_or_else(|| "root".to_string()),
                    message.role,
                    preview(&message.content)
                ),
                SessionEntry::ModelChange {
                    id,
                    parent_id,
                    provider,
                    model_id,
                    ..
                } => format!(
                    "{} <- {}  model_change  {}/{}",
                    short_id(id),
                    parent_id
                        .as_deref()
                        .map(short_id)
                        .unwrap_or_else(|| "root".to_string()),
                    provider,
                    model_id
                ),
                SessionEntry::SessionInfo {
                    id,
                    parent_id,
                    name,
                    ..
                } => format!(
                    "{} <- {}  session_info  {}",
                    short_id(id),
                    parent_id
                        .as_deref()
                        .map(short_id)
                        .unwrap_or_else(|| "root".to_string()),
                    name.as_deref().unwrap_or("-")
                ),
                SessionEntry::Compaction {
                    id,
                    parent_id,
                    summary,
                    tokens_before,
                    ..
                } => format!(
                    "{} <- {}  compaction  {} tokens  {}",
                    short_id(id),
                    parent_id
                        .as_deref()
                        .map(short_id)
                        .unwrap_or_else(|| "root".to_string()),
                    tokens_before,
                    preview(summary)
                ),
                SessionEntry::Label {
                    id,
                    parent_id,
                    target_id,
                    label,
                    ..
                } => format!(
                    "{} <- {}  label {}  {}",
                    short_id(id),
                    parent_id
                        .as_deref()
                        .map(short_id)
                        .unwrap_or_else(|| "root".to_string()),
                    short_id(target_id),
                    label.as_deref().unwrap_or("<cleared>")
                ),
                other => {
                    let kind = match other {
                        SessionEntry::BranchSummary { .. } => "branch_summary",
                        SessionEntry::CustomMessage { .. } => "custom_message",
                        _ => "custom",
                    };
                    format!(
                        "{} <- {}  {kind}",
                        short_id(entry_id(other)),
                        entry_parent_id(other)
                            .map(short_id)
                            .unwrap_or_else(|| "root".to_string()),
                    )
                }
            })
            .collect()
    }

    fn current_branch_entries(&self) -> Vec<SessionEntry> {
        let Some(mut cursor) = self.session.current_node.clone() else {
            return Vec::new();
        };
        let mut branch = Vec::new();
        while let Some(entry) = self.entries.iter().find(|entry| entry_id(entry) == cursor) {
            branch.push(entry.clone());
            let Some(parent_id) = entry_parent_id(entry) else {
                break;
            };
            cursor = parent_id.to_string();
        }
        branch.reverse();
        branch
    }

    fn create(config: &AppConfig) -> Result<Self> {
        Self::create_with_id(config, Uuid::new_v4().to_string())
    }

    fn create_with_id(config: &AppConfig, id: String) -> Result<Self> {
        let created_at = now();
        let session = Session {
            id,
            cwd: config.cwd.clone(),
            name: None,
            current_model: None,
            current_node: None,
            created_at,
        };
        let path = config.session_dir.join(format!("{}.db", session.id));
        Ok(Self {
            session,
            messages: Vec::new(),
            entries: Vec::new(),
            path: Some(path),
        })
    }

    pub fn new_memory(config: &AppConfig, session_id: Option<String>) -> Result<Self> {
        if let Some(session_id) = session_id.as_deref() {
            validate_session_id(session_id)?;
        }
        Ok(Self {
            session: Session {
                id: session_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                cwd: config.cwd.clone(),
                name: None,
                current_model: None,
                current_node: None,
                created_at: now(),
            },
            messages: Vec::new(),
            entries: Vec::new(),
            path: None,
        })
    }

    fn open_path(path: PathBuf) -> Result<Self> {
        let mut session = None;
        let mut messages = Vec::new();
        let mut entries = Vec::new();
        let mut current_node = None;
        let mut current_model = None;
        let mut name = None;

        // Read stored values (SQLite .db or legacy .jsonl), then migrate older
        // formats (v1: add id/parentId + compaction index->id; v2: rename
        // hookMessage role) before deserializing into the strict entry types.
        let mut raw: Vec<Value> = read_store_values(&path)?;
        migrate_session_values(&mut raw);

        for value in raw {
            if value.get("type").and_then(Value::as_str) == Some("session") {
                if let Ok(header) = serde_json::from_value::<SessionHeader>(value) {
                    session = Some(Session {
                        id: header.id,
                        cwd: PathBuf::from(header.cwd),
                        name: None,
                        current_model: None,
                        current_node: None,
                        created_at: header.timestamp,
                    });
                }
                continue;
            }
            // Entries the current port does not model (e.g. custom/hook-message
            // roles) are skipped rather than aborting the whole load.
            let Ok(entry) = serde_json::from_value::<SessionEntry>(value) else {
                continue;
            };
            current_node = Some(entry_id(&entry).to_string());
            match &entry {
                SessionEntry::Message {
                    id,
                    parent_id,
                    timestamp,
                    message,
                } => messages.push(Message {
                    id: id.clone(),
                    parent_id: parent_id.clone(),
                    role: message.role.clone(),
                    content: message.content.clone(),
                    model: current_model.clone(),
                    created_at: timestamp.clone(),
                    images: message.images.clone(),
                    tool_calls: message.tool_calls.clone(),
                    tool_call_id: message.tool_call_id.clone(),
                    tool_name: message.tool_name.clone(),
                    is_error: message.is_error,
                    usage: message.usage.clone(),
                }),
                SessionEntry::ModelChange {
                    provider,
                    model_id,
                    thinking_level,
                    ..
                } => {
                    current_model = Some(model_reference_parts(provider, model_id, *thinking_level))
                }
                SessionEntry::SessionInfo {
                    name: entry_name, ..
                } => name = entry_name.clone(),
                // CustomMessage + BranchSummary participate in LLM context.
                SessionEntry::CustomMessage {
                    id,
                    parent_id,
                    timestamp,
                    content,
                    ..
                } => messages.push(custom_text_message(
                    id,
                    parent_id,
                    timestamp,
                    &content_value_to_text(content),
                    current_model.clone(),
                )),
                SessionEntry::BranchSummary {
                    id,
                    parent_id,
                    timestamp,
                    summary,
                    ..
                } => messages.push(custom_text_message(
                    id,
                    parent_id,
                    timestamp,
                    &format!("[branch summary] {summary}"),
                    current_model.clone(),
                )),
                SessionEntry::Compaction { .. }
                | SessionEntry::Label { .. }
                | SessionEntry::Custom { .. } => {}
            }
            entries.push(entry);
        }

        let mut session = session.ok_or_else(|| anyhow!("session header missing"))?;
        session.name = name;
        session.current_model = current_model;
        session.current_node = current_node;
        Ok(Self {
            session,
            messages,
            entries,
            path: Some(path),
        })
    }

    fn select(session_dir: &Path) -> Result<Self> {
        let sessions = session_files(session_dir)?;
        if sessions.is_empty() {
            bail!("no sessions in {}", session_dir.display());
        }
        for (index, path) in sessions.iter().enumerate() {
            println!("{:>2}. {}", index + 1, path.display());
        }
        print!("Select session: ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let index = input.trim().parse::<usize>().context("invalid selection")?;
        Self::open_path(
            sessions
                .get(index.saturating_sub(1))
                .ok_or_else(|| anyhow!("selection out of range"))?
                .clone(),
        )
    }

    fn rewrite_with_entries(
        &mut self,
        entries: Vec<SessionEntry>,
        parent_session: Option<String>,
    ) -> Result<()> {
        let mut messages = Vec::new();
        let mut current_model = None;
        let mut name = None;
        let mut current_node = None;
        for entry in &entries {
            current_node = Some(entry_id(entry).to_string());
            match entry {
                SessionEntry::Message { .. }
                | SessionEntry::CustomMessage { .. }
                | SessionEntry::BranchSummary { .. } => {
                    if let Some(message) = entry_message(entry, current_model.clone()) {
                        messages.push(message);
                    }
                }
                SessionEntry::ModelChange {
                    provider,
                    model_id,
                    thinking_level,
                    ..
                } => {
                    current_model = Some(model_reference_parts(provider, model_id, *thinking_level))
                }
                SessionEntry::SessionInfo {
                    name: entry_name, ..
                } => name = entry_name.clone(),
                SessionEntry::Compaction { .. }
                | SessionEntry::Label { .. }
                | SessionEntry::Custom { .. } => {}
            }
        }
        self.entries = entries;
        self.messages = messages;
        self.session.current_model = current_model;
        self.session.name = name;
        self.session.current_node = current_node;

        let Some(path) = &self.path else {
            return Ok(());
        };
        let header = SessionHeader {
            entry_type: "session".to_string(),
            version: CURRENT_SESSION_VERSION,
            id: self.session.id.clone(),
            timestamp: self.session.created_at.clone(),
            cwd: self.session.cwd.display().to_string(),
            parent_session,
        };
        let mut values = vec![serde_json::to_value(&header)?];
        for entry in &self.entries {
            values.push(serde_json::to_value(entry)?);
        }
        store_rewrite_values(path, &values)?;
        Ok(())
    }
    fn append_header(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let header = SessionHeader {
            entry_type: "session".to_string(),
            version: CURRENT_SESSION_VERSION,
            id: self.session.id.clone(),
            timestamp: self.session.created_at.clone(),
            cwd: self.session.cwd.display().to_string(),
            parent_session: None,
        };
        store_append_value(path, &serde_json::to_value(&header)?)?;
        Ok(())
    }

    fn append_entry(&mut self, entry: SessionEntry) -> Result<()> {
        self.session.current_node = Some(entry_id(&entry).to_string());
        self.entries.push(entry.clone());
        let Some(path) = &self.path else {
            return Ok(());
        };
        if !path.exists() || store_len(path) == 0 {
            self.append_header()?;
        }
        store_append_value(path, &serde_json::to_value(&entry)?)?;
        Ok(())
    }
}

fn resolve_session_path(input: &str, session_dir: &Path) -> Result<PathBuf> {
    let path = PathBuf::from(input);
    if path.exists() {
        return Ok(path);
    }
    if input.ends_with(".db")
        || input.ends_with(".jsonl")
        || input.contains('/')
        || input.contains('\\')
    {
        // Opening a missing .db path would create an empty store file (SQLite
        // creates on open), so a typo would leave a garbage session behind
        // that a later bare /resume can latch onto. Fail loudly instead.
        bail!("no such session: {}", path.display());
    }
    session_files(session_dir)?
        .into_iter()
        .find(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|id| id == input || id.starts_with(input))
        })
        .ok_or_else(|| anyhow!("no session matching {input}"))
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty() {
        bail!("session id cannot be empty");
    }
    if session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        bail!("session id may only contain ASCII letters, numbers, '-' and '_'")
    }
}

fn latest_session(session_dir: &Path) -> Result<PathBuf> {
    session_files(session_dir)?
        .into_iter()
        .max_by_key(|path| fs::metadata(path).and_then(|m| m.modified()).ok())
        .ok_or_else(|| anyhow!("no sessions in {}", session_dir.display()))
}

/// Keep only the `keep` most-recently-modified sessions; delete older ones
/// (both .db and .jsonl). The currently-open session (`protect_id`) is never
/// deleted, even if it is older than the cutoff (so resuming an old session is
/// safe). Runs at startup for disk hygiene.
pub fn prune_old_sessions(config: &AppConfig, keep: usize, protect_id: &str) {
    let dir = &config.session_dir;
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    // Newest mtime per session id (a .db and its .jsonl backup share a stem).
    let mut by_stem: std::collections::HashMap<String, std::time::SystemTime> =
        std::collections::HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("db") && ext != Some("jsonl") {
            continue;
        }
        if is_input_history_store(&path) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let mtime = fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        by_stem
            .entry(stem.to_string())
            .and_modify(|t| {
                if mtime > *t {
                    *t = mtime;
                }
            })
            .or_insert(mtime);
    }
    let mut stems: Vec<(String, std::time::SystemTime)> = by_stem.into_iter().collect();
    stems.sort_by_key(|(_, time)| std::cmp::Reverse(*time));
    for (stem, _) in stems.into_iter().skip(keep) {
        if stem == protect_id {
            continue;
        }
        let _ = fs::remove_file(dir.join(format!("{stem}.db")));
        let _ = fs::remove_file(dir.join(format!("{stem}.jsonl")));
    }
}

/// The TUI's input-history store lives in the session dir but is NOT a
/// session (bare JSON strings, one per submitted input). Session listing,
/// migration, and pruning must all skip it — treating it as a session panics
/// migrate_v1_to_v2 (`entry["id"]` on a JSON string).
pub const INPUT_HISTORY_FILE: &str = "input-history.jsonl";

fn is_input_history_store(path: &Path) -> bool {
    path.file_stem().and_then(|stem| stem.to_str()) == Some("input-history")
}

fn session_files(session_dir: &Path) -> Result<Vec<PathBuf>> {
    if !session_dir.exists() {
        return Ok(Vec::new());
    }
    // One-time migration: turn each legacy <id>.jsonl into <id>.db (keeping the
    // .jsonl as a backup), so all sessions are SQLite going forward.
    migrate_jsonl_sessions(session_dir);
    let mut files = Vec::new();
    for entry in fs::read_dir(session_dir)? {
        let path = entry?.path();
        if is_input_history_store(&path) {
            continue;
        }
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("db") => files.push(path),
            // Only surface a .jsonl if it has no migrated .db sibling.
            Some("jsonl") if !path.with_extension("db").exists() => files.push(path),
            _ => {}
        }
    }
    files.sort_by_key(|path| std::cmp::Reverse(fs::metadata(path).and_then(|m| m.modified()).ok()));
    Ok(files)
}

/// Convert legacy JSONL sessions to SQLite once (idempotent: skips ones that
/// already have a .db sibling). The original .jsonl is left in place as backup.
fn migrate_jsonl_sessions(session_dir: &Path) {
    let Ok(entries) = fs::read_dir(session_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        if is_input_history_store(&path) {
            continue;
        }
        let db_path = path.with_extension("db");
        if db_path.exists() {
            continue;
        }
        if let Ok(values) = read_store_values(&path) {
            let _ = store_rewrite_values(&db_path, &values);
        }
    }
}

/// Bring older session entries up to the current version, following the
/// migrateToCurrentVersion. v1 -> v2 adds id/parentId tree links and converts a
/// compaction's firstKeptEntryIndex to firstKeptEntryId; v2 -> v3 renames the
/// hookMessage role to custom.
fn migrate_session_values(entries: &mut [Value]) {
    let version = entries
        .iter()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session"))
        .and_then(|value| value.get("version").and_then(Value::as_u64))
        .unwrap_or(1);
    if version >= CURRENT_SESSION_VERSION as u64 {
        return;
    }
    if version < 2 {
        migrate_v1_to_v2(entries);
    }
    if version < 3 {
        migrate_v2_to_v3(entries);
    }
}

fn migrate_v1_to_v2(entries: &mut [Value]) {
    let mut prev: Option<String> = None;
    let mut counter = 0usize;
    for entry in entries.iter_mut() {
        if !entry.is_object() {
            continue;
        }
        if entry.get("type").and_then(Value::as_str) == Some("session") {
            entry["version"] = json!(2);
            continue;
        }
        if entry.get("id").and_then(Value::as_str).is_none() {
            counter += 1;
            entry["id"] = json!(format!("v1m{counter}"));
        }
        let id = entry["id"].as_str().unwrap_or_default().to_string();
        entry["parentId"] = match &prev {
            Some(parent) => json!(parent),
            None => Value::Null,
        };
        prev = Some(id);
    }
    // Second pass: now that every entry has an id, resolve compaction indices.
    let ids: Vec<Option<String>> = entries
        .iter()
        .map(|entry| entry.get("id").and_then(Value::as_str).map(String::from))
        .collect();
    for entry in entries.iter_mut() {
        if entry.get("type").and_then(Value::as_str) != Some("compaction") {
            continue;
        }
        if let Some(index) = entry.get("firstKeptEntryIndex").and_then(Value::as_u64) {
            if let Some(Some(id)) = ids.get(index as usize) {
                entry["firstKeptEntryId"] = json!(id);
            }
            if let Some(object) = entry.as_object_mut() {
                object.remove("firstKeptEntryIndex");
            }
        }
    }
}

fn migrate_v2_to_v3(entries: &mut [Value]) {
    for entry in entries.iter_mut() {
        if entry.get("type").and_then(Value::as_str) == Some("session") {
            entry["version"] = json!(3);
            continue;
        }
        if entry.get("type").and_then(Value::as_str) == Some("message")
            && entry.pointer("/message/role").and_then(Value::as_str) == Some("hookMessage")
        {
            entry["message"]["role"] = json!("custom");
        }
    }
}

/// A short, recognizable label from a session's first substantive user message
/// (skips tiny greetings like "hi"/"안녕" when a longer message follows).
fn session_preview(messages: &[Message]) -> String {
    let user_lines: Vec<String> = messages
        .iter()
        .filter(|message| matches!(message.role, Role::User))
        .filter_map(|message| {
            message
                .content
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.chars().take(50).collect::<String>().replace('\t', " "))
        })
        .collect();
    user_lines
        .iter()
        .find(|line| line.chars().count() >= 6)
        .or_else(|| user_lines.first())
        .cloned()
        .unwrap_or_else(|| "(empty session)".to_string())
}

// ---- Session storage: SQLite (.db) with JSONL (.jsonl) back-compat ----
// A session .db holds one row per former JSONL line in `lines(idx, json)`; the
// rest of the load/tree/branch pipeline is unchanged (it works on Vec<Value>).

fn store_is_db(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("db")
}

fn session_db_conn(path: &Path) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)
        .with_context(|| format!("open session db {}", path.display()))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS lines (idx INTEGER PRIMARY KEY AUTOINCREMENT, json TEXT NOT NULL);",
    )?;
    Ok(conn)
}

/// Read every stored JSON value (header + entries) in order, from either format.
fn read_store_values(path: &Path) -> Result<Vec<Value>> {
    if store_is_db(path) {
        let conn = session_db_conn(path)?;
        let mut stmt = conn.prepare("SELECT json FROM lines ORDER BY idx")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|line| {
                serde_json::from_str(&line)
                    .with_context(|| format!("invalid session row in {}", path.display()))
            })
            .collect()
    } else {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read session {}", path.display()))?;
        text.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| {
                serde_json::from_str(line)
                    .with_context(|| format!("invalid JSONL {}:{}", path.display(), index + 1))
            })
            .collect()
    }
}

fn store_len(path: &Path) -> usize {
    if store_is_db(path) {
        session_db_conn(path)
            .and_then(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM lines", [], |row| row.get::<_, i64>(0))?)
            })
            .unwrap_or(0) as usize
    } else {
        fs::metadata(path)
            .map(|meta| if meta.len() == 0 { 0 } else { 1 })
            .unwrap_or(0)
    }
}

fn store_append_value(path: &Path, value: &Value) -> Result<()> {
    if store_is_db(path) {
        let conn = session_db_conn(path)?;
        conn.execute(
            "INSERT INTO lines (json) VALUES (?1)",
            rusqlite::params![value.to_string()],
        )?;
    } else {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(value)?)?;
    }
    Ok(())
}

fn store_rewrite_values(path: &Path, values: &[Value]) -> Result<()> {
    if store_is_db(path) {
        let mut conn = session_db_conn(path)?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM lines", [])?;
        for value in values {
            tx.execute(
                "INSERT INTO lines (json) VALUES (?1)",
                rusqlite::params![value.to_string()],
            )?;
        }
        tx.commit()?;
    } else {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        for value in values {
            writeln!(file, "{}", serde_json::to_string(value)?)?;
        }
    }
    Ok(())
}

fn entry_id(entry: &SessionEntry) -> &str {
    match entry {
        SessionEntry::Message { id, .. }
        | SessionEntry::ModelChange { id, .. }
        | SessionEntry::SessionInfo { id, .. }
        | SessionEntry::Compaction { id, .. }
        | SessionEntry::Label { id, .. }
        | SessionEntry::BranchSummary { id, .. }
        | SessionEntry::Custom { id, .. }
        | SessionEntry::CustomMessage { id, .. } => id,
    }
}

fn entry_parent_id(entry: &SessionEntry) -> Option<&str> {
    match entry {
        SessionEntry::Message { parent_id, .. }
        | SessionEntry::ModelChange { parent_id, .. }
        | SessionEntry::SessionInfo { parent_id, .. }
        | SessionEntry::Compaction { parent_id, .. }
        | SessionEntry::Label { parent_id, .. }
        | SessionEntry::BranchSummary { parent_id, .. }
        | SessionEntry::Custom { parent_id, .. }
        | SessionEntry::CustomMessage { parent_id, .. } => parent_id.as_deref(),
    }
}

fn reparent_entry(entry: SessionEntry, parent_id: Option<String>) -> SessionEntry {
    match entry {
        SessionEntry::Message {
            id,
            timestamp,
            message,
            ..
        } => SessionEntry::Message {
            id,
            parent_id,
            timestamp,
            message,
        },
        SessionEntry::ModelChange {
            id,
            timestamp,
            provider,
            model_id,
            thinking_level,
            ..
        } => SessionEntry::ModelChange {
            id,
            parent_id,
            timestamp,
            provider,
            model_id,
            thinking_level,
        },
        SessionEntry::SessionInfo {
            id,
            timestamp,
            name,
            ..
        } => SessionEntry::SessionInfo {
            id,
            parent_id,
            timestamp,
            name,
        },
        SessionEntry::Compaction {
            id,
            timestamp,
            summary,
            first_kept_entry_id,
            tokens_before,
            ..
        } => SessionEntry::Compaction {
            id,
            parent_id,
            timestamp,
            summary,
            first_kept_entry_id,
            tokens_before,
        },
        SessionEntry::Label {
            id,
            timestamp,
            target_id,
            label,
            ..
        } => SessionEntry::Label {
            id,
            parent_id,
            timestamp,
            target_id,
            label,
        },
        SessionEntry::BranchSummary {
            id,
            timestamp,
            summary,
            from_id,
            details,
            from_hook,
            ..
        } => SessionEntry::BranchSummary {
            id,
            parent_id,
            timestamp,
            summary,
            from_id,
            details,
            from_hook,
        },
        SessionEntry::Custom {
            id,
            timestamp,
            custom_type,
            data,
            ..
        } => SessionEntry::Custom {
            id,
            parent_id,
            timestamp,
            custom_type,
            data,
        },
        SessionEntry::CustomMessage {
            id,
            timestamp,
            custom_type,
            content,
            display,
            details,
            ..
        } => SessionEntry::CustomMessage {
            id,
            parent_id,
            timestamp,
            custom_type,
            content,
            display,
            details,
        },
    }
}

fn model_reference(model: &Model, thinking: Option<ThinkingLevel>) -> String {
    model_reference_parts(&model.provider, &model.id, thinking)
}

fn model_reference_parts(
    provider: &str,
    model_id: &str,
    thinking: Option<ThinkingLevel>,
) -> String {
    match thinking {
        Some(level) => format!("{provider}/{model_id}:{}", level.as_str()),
        None => format!("{provider}/{model_id}"),
    }
}

fn entry_message(entry: &SessionEntry, model: Option<String>) -> Option<Message> {
    match entry {
        SessionEntry::Message {
            id,
            parent_id,
            timestamp,
            message,
        } => Some(Message {
            id: id.clone(),
            parent_id: parent_id.clone(),
            role: message.role.clone(),
            content: message.content.clone(),
            model,
            created_at: timestamp.clone(),
            images: message.images.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
            tool_name: message.tool_name.clone(),
            is_error: message.is_error,
            usage: message.usage.clone(),
        }),
        // Extension-injected and branch-summary entries participate in context.
        SessionEntry::CustomMessage {
            id,
            parent_id,
            timestamp,
            content,
            ..
        } => Some(custom_text_message(
            id,
            parent_id,
            timestamp,
            &content_value_to_text(content),
            model,
        )),
        SessionEntry::BranchSummary {
            id,
            parent_id,
            timestamp,
            summary,
            ..
        } => Some(custom_text_message(
            id,
            parent_id,
            timestamp,
            &format!("[branch summary] {summary}"),
            model,
        )),
        _ => None,
    }
}

fn new_entry_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn preview(content: &str) -> String {
    content
        .replace('\n', " ")
        .chars()
        .take(80)
        .collect::<String>()
}

fn estimate_tokens(content: &str) -> usize {
    content.len().div_ceil(4)
}

const EXPORT_HTML_CSS: &str = r#"
:root { color-scheme: light; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #f6f7f9; color: #1f2933; }
* { box-sizing: border-box; }
body { margin: 0; }
header { display: grid; grid-template-columns: minmax(0, 1fr) minmax(280px, 520px); gap: 24px; padding: 32px; background: #ffffff; border-bottom: 1px solid #d9dee5; }
h1, h2, h3, p { margin: 0; }
h1 { font-size: 28px; line-height: 1.2; font-weight: 700; }
h2 { font-size: 18px; margin-bottom: 14px; }
.eyebrow { font-size: 12px; text-transform: uppercase; letter-spacing: .08em; color: #596574; margin-bottom: 8px; }
.meta { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 8px 14px; margin: 0; font-size: 13px; }
.meta dt { color: #596574; font-weight: 600; }
.meta dd { margin: 0; overflow-wrap: anywhere; }
main { max-width: 1180px; margin: 0 auto; padding: 28px 20px 44px; display: grid; gap: 28px; }
section { min-width: 0; }
.tree, .conversation { display: grid; gap: 10px; }
.entry, .message { background: #ffffff; border: 1px solid #d9dee5; border-radius: 8px; padding: 14px; }
.entry-header, .message-header { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; font-size: 13px; color: #596574; }
.badge { display: inline-flex; align-items: center; min-height: 24px; padding: 2px 8px; border: 1px solid #c8d0da; border-radius: 999px; color: #344054; background: #f8fafc; font-weight: 600; }
.id { font-family: ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace; color: #344054; }
pre { margin: 12px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; font-family: ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace; font-size: 13px; line-height: 1.5; }
details { margin-top: 10px; }
summary { cursor: pointer; color: #344054; font-weight: 600; }
.assistant { border-left: 4px solid #64748b; }
.user { border-left: 4px solid #2563eb; }
.tool { border-left: 4px solid #7c3aed; }
.error { border-left-color: #dc2626; }
@media (max-width: 760px) { header { grid-template-columns: 1fr; padding: 22px 18px; } main { padding: 20px 14px 34px; } }
"#;

fn push_meta(html: &mut String, key: &str, value: &str) {
    html.push_str("<dt>");
    html.push_str(&html_escape(key));
    html.push_str("</dt><dd>");
    html.push_str(&html_escape(value));
    html.push_str("</dd>");
}

fn push_html_entry(html: &mut String, entry: &SessionEntry) {
    match entry {
        SessionEntry::Message {
            id,
            parent_id,
            timestamp,
            message,
        } => {
            let role = role_label(&message.role);
            html.push_str("<article class=\"entry ");
            html.push_str(role);
            if message.is_error {
                html.push_str(" error");
            }
            html.push_str("\"><div class=\"entry-header\"><span class=\"badge\">message:");
            html.push_str(role);
            html.push_str("</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("</div><pre>");
            html.push_str(&html_escape(&message.content));
            html.push_str("</pre>");
            if !message.tool_calls.is_empty() {
                push_tool_calls(html, &message.tool_calls);
            }
            if let Some(tool_call_id) = &message.tool_call_id {
                html.push_str("<p class=\"entry-header\"><span class=\"badge\">tool result</span><span class=\"id\">");
                html.push_str(&html_escape(tool_call_id));
                html.push_str("</span>");
                if let Some(tool_name) = &message.tool_name {
                    html.push_str("<span>");
                    html.push_str(&html_escape(tool_name));
                    html.push_str("</span>");
                }
                html.push_str("</p>");
            }
            html.push_str("</article>");
        }
        SessionEntry::ModelChange {
            id,
            parent_id,
            timestamp,
            provider,
            model_id,
            ..
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">model_change</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("</div><pre>");
            html.push_str(&html_escape(&format!("{provider}/{model_id}")));
            html.push_str("</pre></article>");
        }
        SessionEntry::SessionInfo {
            id,
            parent_id,
            timestamp,
            name,
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">session_info</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("</div><pre>");
            html.push_str(&html_escape(name.as_deref().unwrap_or("-")));
            html.push_str("</pre></article>");
        }
        SessionEntry::Compaction {
            id,
            parent_id,
            timestamp,
            summary,
            first_kept_entry_id,
            tokens_before,
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">compaction</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("<span>");
            html.push_str(&html_escape(&format!("{tokens_before} tokens")));
            html.push_str("</span><span>keeps ");
            html.push_str(&html_escape(first_kept_entry_id));
            html.push_str("</span></div><pre>");
            html.push_str(&html_escape(summary));
            html.push_str("</pre></article>");
        }
        SessionEntry::Label {
            id,
            parent_id,
            timestamp,
            target_id,
            label,
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">label</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("<span>target ");
            html.push_str(&html_escape(target_id));
            html.push_str("</span></div><pre>");
            html.push_str(&html_escape(label.as_deref().unwrap_or("<cleared>")));
            html.push_str("</pre></article>");
        }
        SessionEntry::BranchSummary {
            id,
            parent_id,
            timestamp,
            summary,
            ..
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">branch_summary</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("</div><pre>");
            html.push_str(&html_escape(summary));
            html.push_str("</pre></article>");
        }
        SessionEntry::CustomMessage {
            id,
            parent_id,
            timestamp,
            custom_type,
            content,
            ..
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">custom_message:");
            html.push_str(&html_escape(custom_type));
            html.push_str("</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("</div><pre>");
            html.push_str(&html_escape(&content_value_to_text(content)));
            html.push_str("</pre></article>");
        }
        SessionEntry::Custom {
            id,
            parent_id,
            timestamp,
            custom_type,
            ..
        } => {
            html.push_str("<article class=\"entry\"><div class=\"entry-header\"><span class=\"badge\">custom:");
            html.push_str(&html_escape(custom_type));
            html.push_str("</span>");
            push_entry_identity(html, id, parent_id.as_deref(), timestamp);
            html.push_str("</div></article>");
        }
    }
}

fn push_html_message(html: &mut String, message: &Message) {
    let role = role_label(&message.role);
    html.push_str("<article class=\"message ");
    html.push_str(role);
    if message.is_error {
        html.push_str(" error");
    }
    html.push_str("\"><div class=\"message-header\"><span class=\"badge\">");
    html.push_str(role);
    html.push_str("</span><span class=\"id\">");
    html.push_str(&html_escape(&message.id));
    html.push_str("</span><span>&lt;- ");
    html.push_str(&html_escape(message.parent_id.as_deref().unwrap_or("root")));
    html.push_str("</span><span>");
    html.push_str(&html_escape(&message.created_at));
    html.push_str("</span>");
    if let Some(model) = &message.model {
        html.push_str("<span>");
        html.push_str(&html_escape(model));
        html.push_str("</span>");
    }
    html.push_str("</div><pre>");
    html.push_str(&html_escape(&message.content));
    html.push_str("</pre>");
    if !message.tool_calls.is_empty() {
        push_tool_calls(html, &message.tool_calls);
    }
    html.push_str("</article>");
}

fn push_entry_identity(html: &mut String, id: &str, parent_id: Option<&str>, timestamp: &str) {
    html.push_str("<span class=\"id\">");
    html.push_str(&html_escape(id));
    html.push_str("</span><span>&lt;- ");
    html.push_str(&html_escape(parent_id.unwrap_or("root")));
    html.push_str("</span><span>");
    html.push_str(&html_escape(timestamp));
    html.push_str("</span>");
}

fn push_tool_calls(html: &mut String, calls: &[ToolCallRecord]) {
    html.push_str("<details><summary>Tool calls</summary>");
    for call in calls {
        html.push_str("<pre>");
        html.push_str(&html_escape(&format!(
            "{} {}\n{}",
            call.id,
            call.name,
            serde_json::to_string_pretty(&call.arguments)
                .unwrap_or_else(|_| call.arguments.to_string())
        )));
        html.push_str("</pre>");
    }
    html.push_str("</details>");
}

fn role_label(role: &Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pi_entry_types_round_trip() {
        let lines = [
            r#"{"type":"branch_summary","id":"g7","parentId":"a1","timestamp":"t","summary":"explored A","fromId":"f6"}"#,
            r#"{"type":"custom","id":"h8","parentId":"g7","timestamp":"t","customType":"my-ext","data":{"count":42}}"#,
            r#"{"type":"custom_message","id":"i9","parentId":"h8","timestamp":"t","customType":"my-ext","content":"injected","display":true}"#,
        ];
        for line in lines {
            let entry: SessionEntry = serde_json::from_str(line).expect("parse pi entry");
            let back: Value =
                serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
            let orig: Value = serde_json::from_str(line).unwrap();
            assert_eq!(back["type"], orig["type"]);
            assert_eq!(entry_id(&entry), orig["id"].as_str().unwrap());
            assert_eq!(entry_parent_id(&entry), orig["parentId"].as_str());
        }
        // custom_message + branch_summary participate in context; custom does not.
        let cm: SessionEntry = serde_json::from_str(lines[2]).unwrap();
        assert!(entry_message(&cm, None).is_some());
        let bs: SessionEntry = serde_json::from_str(lines[0]).unwrap();
        assert!(entry_message(&bs, None).is_some());
        let c: SessionEntry = serde_json::from_str(lines[1]).unwrap();
        assert!(entry_message(&c, None).is_none());
    }

    #[test]
    fn migrate_v1_skips_non_object_entries() {
        // Bare-string lines (e.g. the input-history store) must never panic
        // the id/parentId migration.
        let mut entries = vec![
            json!("안녕! 히스토리 라인"),
            json!({"type":"message","timestamp":"t","message":{"role":"user","content":"hi"}}),
        ];
        migrate_session_values(&mut entries);
        assert_eq!(entries[0], json!("안녕! 히스토리 라인"));
        assert_eq!(entries[1]["id"].as_str(), Some("v1m1"));
    }

    #[test]
    fn session_files_ignores_input_history_store() {
        let dir = std::env::temp_dir().join("bbarit-session-input-history");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(INPUT_HISTORY_FILE), "\"hello\"\n").unwrap();
        let files = session_files(&dir).unwrap();
        assert!(files.is_empty());
        // The legacy-JSONL sweep must not convert it into a fake session db.
        assert!(!dir.join("input-history.db").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    fn write_session(name: &str, lines: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bbarit-session-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(lines.join("\n").as_bytes()).unwrap();
        path
    }

    #[test]
    fn migrates_v1_session_without_ids() {
        // v1: no version field, no id/parentId on entries.
        let lines = [
            r#"{"type":"session","id":"s1","timestamp":"t","cwd":"/tmp"}"#,
            r#"{"type":"message","timestamp":"t","message":{"role":"user","content":"hello"}}"#,
            r#"{"type":"message","timestamp":"t","message":{"role":"assistant","content":"hi"}}"#,
        ];
        let path = write_session("v1", &lines);
        let store = SessionStore::open_path(path).unwrap();
        let convo: Vec<String> = store
            .conversation()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(convo, vec!["hello", "hi"]);
    }

    #[test]
    fn migrates_v2_hookmessage_role() {
        // v2 -> v3 renames hookMessage to custom, which this port skips.
        let lines = [
            r#"{"type":"session","version":2,"id":"s1","timestamp":"t","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"A","parentId":null,"timestamp":"t","message":{"role":"user","content":"keep"}}"#,
            r#"{"type":"message","id":"H","parentId":"A","timestamp":"t","message":{"role":"hookMessage","content":"drop"}}"#,
        ];
        let path = write_session("v2hook", &lines);
        let store = SessionStore::open_path(path).unwrap();
        let convo: Vec<String> = store
            .conversation()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(convo, vec!["keep"]);
    }

    #[test]
    fn conversation_follows_active_branch() {
        let lines = [
            r#"{"type":"session","version":3,"id":"s1","timestamp":"t","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"A","parentId":null,"timestamp":"t","message":{"role":"user","content":"hello"}}"#,
            r#"{"type":"message","id":"B","parentId":"A","timestamp":"t","message":{"role":"assistant","content":"hi"}}"#,
            r#"{"type":"message","id":"C","parentId":"B","timestamp":"t","message":{"role":"user","content":"branch-one"}}"#,
            r#"{"type":"message","id":"D","parentId":"B","timestamp":"t","message":{"role":"user","content":"branch-two"}}"#,
        ];
        let path = write_session("branch", &lines);
        let mut store = SessionStore::open_path(path).unwrap();
        // current_node defaults to the last entry in file order (D).
        let convo: Vec<String> = store
            .conversation()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(convo, vec!["hello", "hi", "branch-two"]);
        // Switching the head to C selects the other branch.
        store.session.current_node = Some("C".to_string());
        let convo: Vec<String> = store
            .conversation()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(convo, vec!["hello", "hi", "branch-one"]);
    }

    #[test]
    fn resolve_session_path_rejects_missing_explicit_path() {
        let dir = std::env::temp_dir().join("bbarit-session-resolve-missing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("typo.db");
        let err = resolve_session_path(missing.to_str().unwrap(), &dir).unwrap_err();
        assert!(err.to_string().contains("no such session"));
        // The typo'd path must not have been created as a garbage store.
        assert!(!missing.exists());
        let err = resolve_session_path("typo.jsonl", &dir).unwrap_err();
        assert!(err.to_string().contains("no such session"));
    }

    #[test]
    fn import_does_not_overwrite_existing_session_with_same_basename() {
        let dir = std::env::temp_dir().join("bbarit-session-import-conflict");
        let _ = fs::remove_dir_all(&dir);
        let session_dir = dir.join("sessions");
        let source_dir = dir.join("elsewhere");
        fs::create_dir_all(&session_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();
        let existing = session_dir.join("s.jsonl");
        fs::write(
            &existing,
            r#"{"type":"session","version":3,"id":"old","timestamp":"t","cwd":"/tmp"}"#,
        )
        .unwrap();
        let source = source_dir.join("s.jsonl");
        fs::write(
            &source,
            r#"{"type":"session","version":3,"id":"new","timestamp":"t","cwd":"/tmp"}"#,
        )
        .unwrap();
        let mut config = AppConfig::for_test(dir.clone());
        config.session_dir = session_dir.clone();
        let store = SessionStore::import_jsonl(&config, &source).unwrap();
        assert_eq!(store.session().id, "new");
        assert_eq!(store.session_file().unwrap(), session_dir.join("s-1.jsonl"));
        // The existing session with the same basename is untouched.
        let old = SessionStore::open_path(existing).unwrap();
        assert_eq!(old.session().id, "old");
    }

    #[test]
    fn import_rejects_unparseable_source_without_copying() {
        let dir = std::env::temp_dir().join("bbarit-session-import-invalid");
        let _ = fs::remove_dir_all(&dir);
        let session_dir = dir.join("sessions");
        let source_dir = dir.join("elsewhere");
        fs::create_dir_all(&session_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("bad.jsonl");
        fs::write(&source, "not a session").unwrap();
        let mut config = AppConfig::for_test(dir.clone());
        config.session_dir = session_dir.clone();
        let err = SessionStore::import_jsonl(&config, &source)
            .err()
            .expect("import of unparseable source must fail");
        assert!(err.to_string().contains("not a valid session file"));
        assert!(!session_dir.join("bad.jsonl").exists());
    }

    #[test]
    fn ambiguous_entry_prefix_fails_loudly() {
        let lines = [
            r#"{"type":"session","version":3,"id":"s1","timestamp":"t","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"AB1","parentId":null,"timestamp":"t","message":{"role":"user","content":"one"}}"#,
            r#"{"type":"message","id":"AB2","parentId":"AB1","timestamp":"t","message":{"role":"assistant","content":"two"}}"#,
        ];
        let path = write_session("ambiguous-prefix", &lines);
        let mut store = SessionStore::open_path(path).unwrap();
        let err = store.new_branch_at("AB").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("ambiguous"));
        assert!(message.contains("AB1") && message.contains("AB2"));
        // A unique prefix still resolves.
        store.new_branch_at("AB1").unwrap();
        assert_eq!(store.session().current_node.as_deref(), Some("AB1"));
    }

    #[test]
    fn clone_preserves_branched_head() {
        let lines = [
            r#"{"type":"session","version":3,"id":"s1","timestamp":"t","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"A","parentId":null,"timestamp":"t","message":{"role":"user","content":"hello"}}"#,
            r#"{"type":"message","id":"B","parentId":"A","timestamp":"t","message":{"role":"assistant","content":"hi"}}"#,
            r#"{"type":"message","id":"C","parentId":"B","timestamp":"t","message":{"role":"user","content":"tip"}}"#,
        ];
        let path = write_session("clone-head", &lines);
        let mut store = SessionStore::open_path(path).unwrap();
        store.new_branch_at("A").unwrap();
        let dir = std::env::temp_dir().join("bbarit-session-clone-head-out");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = AppConfig::for_test(dir);
        let cloned = store.clone_current(&config).unwrap();
        assert_eq!(cloned.session().current_node.as_deref(), Some("A"));
        let convo: Vec<String> = cloned
            .conversation()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(convo, vec!["hello"]);
    }

    #[test]
    fn compaction_keeps_messages_on_active_branch() {
        let lines = [
            r#"{"type":"session","version":3,"id":"s1","timestamp":"t","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"A","parentId":null,"timestamp":"t","message":{"role":"user","content":"hello"}}"#,
            r#"{"type":"message","id":"B","parentId":"A","timestamp":"t","message":{"role":"assistant","content":"hi"}}"#,
            r#"{"type":"message","id":"C","parentId":"B","timestamp":"t","message":{"role":"user","content":"branch-one"}}"#,
            r#"{"type":"message","id":"D","parentId":"B","timestamp":"t","message":{"role":"user","content":"branch-two"}}"#,
        ];
        let path = write_session("compact-branch", &lines);
        let mut store = SessionStore::open_path(path).unwrap();
        // Head is D; C sits on an abandoned branch between the kept messages
        // in file order, so a flat-list pick would land on C and drop the
        // kept messages from context.
        store.append_compaction("sum", 2).unwrap();
        let convo: Vec<String> = store
            .conversation()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert!(convo[0].contains("sum"));
        assert_eq!(&convo[1..], ["hi", "branch-two"]);
    }
}
