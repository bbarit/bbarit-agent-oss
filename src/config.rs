use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::cli::Cli;

pub const APP_DIR: &str = ".bbarit-oss";
/// The OSS agent's OWN home root. Deliberately distinct from the BBARIT
/// Terminal app's `~/.bbarit` so a standalone install never shares logins,
/// memories, sessions, or settings with the desktop app.
pub const USER_APP_ROOT: &str = ".bbarit-oss";
pub const PI_AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
pub const PI_SESSION_DIR_ENV: &str = "PI_CODING_AGENT_SESSION_DIR";
pub const PI_PACKAGE_DIR_ENV: &str = "PI_PACKAGE_DIR";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub cwd: PathBuf,
    pub app_dir: PathBuf,
    pub user_app_dir: PathBuf,
    pub project_trusted: bool,
    pub project_resources_detected: bool,
    pub session_dir: PathBuf,
    pub provider: String,
    pub model: Option<String>,
    pub thinking_level: Option<crate::providers::ThinkingLevel>,
    /// Persona adopted at startup when no --persona/env override is given.
    pub default_persona: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Vec<String>,
    pub context_files: Vec<ContextFile>,
    pub favorites: Vec<String>,
    pub no_tools: bool,
    pub no_builtin_tools: bool,
    pub tool_allowlist: Vec<String>,
    pub tool_exclude: Vec<String>,
    pub no_extensions: bool,
    pub no_skills: bool,
    pub no_prompt_templates: bool,
    pub no_context_files: bool,
    pub no_themes: bool,
    pub extension_paths: Vec<PathBuf>,
    pub packages: Vec<PackageSpec>,
    pub prompt_paths: Vec<ResourcePathSpec>,
    pub skill_paths: Vec<ResourcePathSpec>,
    pub theme_paths: Vec<PathBuf>,
    pub enable_skill_commands: bool,
    pub shell_path: Option<String>,
    pub shell_command_prefix: Option<String>,
    pub npm_command: Option<Vec<String>>,
    pub auth_paths: Vec<PathBuf>,
    pub compaction_enabled: bool,
    pub compaction_reserve_tokens: usize,
    pub compaction_keep_recent_tokens: usize,
    pub retry_max_retries: usize,
    pub stream: bool,
    pub review_model: Option<String>,
}

/// Sensible defaults for compaction / retry.
pub const DEFAULT_COMPACTION_RESERVE_TOKENS: usize = 16_384;
pub const DEFAULT_COMPACTION_KEEP_RECENT_TOKENS: usize = 20_000;
pub const DEFAULT_RETRY_MAX_RETRIES: usize = 2;

#[cfg(test)]
impl AppConfig {
    /// Minimal config for unit tests: every resource discovery is disabled and
    /// all directories point at `cwd`, so behavior is deterministic.
    pub(crate) fn for_test(cwd: PathBuf) -> Self {
        AppConfig {
            cwd: cwd.clone(),
            app_dir: cwd.clone(),
            user_app_dir: cwd.clone(),
            project_trusted: true,
            project_resources_detected: false,
            session_dir: cwd,
            provider: "anthropic".to_string(),
            model: None,
            thinking_level: None,
            default_persona: None,
            api_key: None,
            system_prompt: None,
            append_system_prompt: Vec::new(),
            context_files: Vec::new(),
            favorites: Vec::new(),
            no_tools: false,
            no_builtin_tools: false,
            tool_allowlist: Vec::new(),
            tool_exclude: Vec::new(),
            no_extensions: true,
            no_skills: true,
            no_prompt_templates: true,
            no_context_files: false,
            no_themes: true,
            extension_paths: Vec::new(),
            packages: Vec::new(),
            prompt_paths: Vec::new(),
            skill_paths: Vec::new(),
            theme_paths: Vec::new(),
            enable_skill_commands: false,
            shell_path: None,
            shell_command_prefix: None,
            npm_command: None,
            auth_paths: Vec::new(),
            compaction_enabled: true,
            compaction_reserve_tokens: DEFAULT_COMPACTION_RESERVE_TOKENS,
            compaction_keep_recent_tokens: DEFAULT_COMPACTION_KEEP_RECENT_TOKENS,
            retry_max_retries: DEFAULT_RETRY_MAX_RETRIES,
            stream: false,
            review_model: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ResourcePathSpec {
    pub base_dir: PathBuf,
    pub value: String,
}

impl ResourcePathSpec {
    pub fn resolved_path(&self) -> PathBuf {
        resolve_config_path(&self.base_dir, self.value.clone())
    }

    pub fn display_path(&self) -> String {
        let (prefix, value) = split_resource_pattern_prefix(&self.value);
        let path = resolve_config_path(&self.base_dir, value.to_string());
        format!("{prefix}{}", path.display())
    }
}

#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub base_dir: PathBuf,
    pub source: String,
    pub extensions: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub themes: Option<Vec<String>>,
}

impl PackageSpec {
    pub fn resolved_root(&self) -> PathBuf {
        resolved_package_root(&self.base_dir, &self.source)
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    default_provider: Option<String>,
    default_model: Option<String>,
    default_thinking_level: Option<String>,
    default_persona: Option<String>,
    enabled_models: Option<Vec<String>>,
    session_dir: Option<String>,
    system_prompt: Option<String>,
    append_system_prompt: Option<Vec<String>>,
    packages: Option<Vec<PackageSetting>>,
    extensions: Option<Vec<String>>,
    prompts: Option<Vec<String>>,
    skills: Option<SkillsSetting>,
    themes: Option<Vec<String>>,
    enable_skill_commands: Option<bool>,
    shell_path: Option<String>,
    shell_command_prefix: Option<String>,
    npm_command: Option<Vec<String>>,
    default_project_trust: Option<String>,
    compaction: Option<CompactionSetting>,
    retry: Option<RetrySetting>,
    stream: Option<bool>,
    review_model: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct CompactionSetting {
    enabled: Option<bool>,
    reserve_tokens: Option<usize>,
    keep_recent_tokens: Option<usize>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RetrySetting {
    max_retries: Option<usize>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum PackageSetting {
    Source(String),
    Filtered(PackageFilterSetting),
}

#[derive(Debug, Deserialize, Clone)]
struct PackageFilterSetting {
    source: String,
    extensions: Option<StringList>,
    skills: Option<StringList>,
    prompts: Option<StringList>,
    themes: Option<StringList>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum StringList {
    One(String),
    Many(Vec<String>),
}

impl StringList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum SkillsSetting {
    Paths(Vec<String>),
    Legacy(LegacySkillsSetting),
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LegacySkillsSetting {
    enable_skill_commands: Option<bool>,
    custom_directories: Option<Vec<String>>,
}

impl SkillsSetting {
    fn into_paths(self) -> Vec<String> {
        match self {
            Self::Paths(paths) => paths,
            Self::Legacy(legacy) => legacy.custom_directories.unwrap_or_default(),
        }
    }

    fn enable_skill_commands(&self) -> Option<bool> {
        match self {
            Self::Paths(_) => None,
            Self::Legacy(legacy) => legacy.enable_skill_commands,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ModelsJson {
    #[serde(default)]
    pub providers: BTreeMap<String, CustomProvider>,
}

#[derive(Debug, Deserialize)]
pub struct CustomProvider {
    pub name: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "apiKeyEnv")]
    pub api_key_env: Option<String>,
    pub api: Option<String>,
    #[serde(default)]
    pub models: Vec<CustomModel>,
    #[serde(default, rename = "modelOverrides")]
    pub model_overrides: BTreeMap<String, ModelOverride>,
}

#[derive(Debug, Deserialize)]
pub struct CustomModel {
    pub id: String,
    pub name: Option<String>,
    pub api: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "contextWindow")]
    pub context_window: Option<u32>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ModelOverride {
    pub name: Option<String>,
    pub api: Option<String>,
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    pub reasoning: Option<bool>,
    #[serde(rename = "contextWindow")]
    pub context_window: Option<u32>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<u32>,
}

fn resolve_startup_cwd() -> Result<PathBuf> {
    match env::current_dir() {
        Ok(cwd) => Ok(cwd),
        Err(error) => {
            if let Some(cwd) = env::var_os("BBARIT_AGENT_CWD")
                .or_else(|| env::var_os("PWD"))
                .map(PathBuf::from)
                .filter(|path| path.is_dir())
            {
                return Ok(cwd);
            }
            Err(error).context("failed to resolve cwd")
        }
    }
}

impl AppConfig {
    pub fn load(cli: &Cli) -> Result<Self> {
        let cwd = resolve_startup_cwd()?;
        let app_dir = cwd.join(APP_DIR);
        let home = dirs_next::home_dir().unwrap_or_else(|| cwd.clone());
        let user_app_dir = env::var(PI_AGENT_DIR_ENV)
            .ok()
            .map(resolve_home_path)
            .unwrap_or_else(|| home.join(USER_APP_ROOT).join("agent"));
        let global_settings = read_merged_settings([user_app_dir.join("settings.json")])?;
        let project_resources_detected = has_project_resources(&cwd, &app_dir);
        let project_trusted = resolve_project_trusted(
            cli,
            &cwd,
            &user_app_dir,
            project_resources_detected,
            global_settings.default_project_trust.as_deref(),
        )?;
        let project_settings = if project_trusted {
            read_merged_settings([app_dir.join("settings.json")])?
        } else {
            Settings::default()
        };
        let session_dir = cli
            .session_dir
            .clone()
            .map(resolve_pathbuf_home)
            .or_else(|| env::var(PI_SESSION_DIR_ENV).ok().map(resolve_home_path))
            .or_else(|| env::var("BBARIT_SESSION_DIR").ok().map(resolve_home_path))
            .or_else(|| {
                project_settings
                    .session_dir
                    .as_ref()
                    .map(|path| resolve_config_path(&cwd, path.clone()))
            })
            .or_else(|| {
                global_settings
                    .session_dir
                    .as_ref()
                    .map(|path| resolve_config_path(&cwd, path.clone()))
            })
            .unwrap_or_else(|| user_app_dir.join("sessions"));
        let thinking_level = if let Some(value) = cli.thinking.as_deref() {
            Some(crate::providers::ThinkingLevel::parse(value)?)
        } else if let Some(value) = project_settings
            .default_thinking_level
            .as_deref()
            .or(global_settings.default_thinking_level.as_deref())
        {
            Some(crate::providers::ThinkingLevel::parse(value)?)
        } else {
            None
        };
        let default_persona = project_settings
            .default_persona
            .clone()
            .or_else(|| global_settings.default_persona.clone());
        let favorites = cli
            .favorite_models
            .as_deref()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .or_else(|| project_settings.enabled_models.clone())
            .or_else(|| global_settings.enabled_models.clone())
            .unwrap_or_else(|| {
                vec![
                    "anthropic/claude-opus-4-8".to_string(),
                    "openai/gpt-5.6-sol".to_string(),
                    "google/gemini-3.1-pro-preview".to_string(),
                    "kimi-coding/k3".to_string(),
                ]
            });
        let mut append_system_prompt = Vec::new();
        for value in global_settings.append_system_prompt.unwrap_or_default() {
            append_system_prompt.push(resolve_prompt_input(&user_app_dir, value)?);
        }
        for value in project_settings.append_system_prompt.unwrap_or_default() {
            append_system_prompt.push(resolve_prompt_input(&app_dir, value)?);
        }
        for value in cli.append_system_prompt.clone() {
            append_system_prompt.push(resolve_prompt_input(&cwd, value)?);
        }
        if append_system_prompt.is_empty()
            && let Some(path) =
                discover_append_system_prompt_file(&app_dir, &user_app_dir, project_trusted)
        {
            append_system_prompt.push(fs::read_to_string(&path).with_context(|| {
                format!("failed to read append system prompt {}", path.display())
            })?);
        }
        let tool_allowlist = parse_tool_list(cli.tools.as_deref());
        let tool_exclude = parse_tool_list(cli.exclude_tools.as_deref());
        let settings_extensions = if cli.no_extensions {
            Vec::new()
        } else {
            [
                resolve_config_paths(
                    &user_app_dir,
                    global_settings.extensions.clone().unwrap_or_default(),
                ),
                resolve_config_paths(
                    &app_dir,
                    project_settings.extensions.clone().unwrap_or_default(),
                ),
            ]
            .concat()
        };
        let extension_paths = [
            settings_extensions,
            resolve_cli_paths(&cwd, cli.extensions.clone()),
        ]
        .concat();
        let packages = [
            package_specs(
                &user_app_dir,
                global_settings.packages.clone().unwrap_or_default(),
            ),
            package_specs(
                &app_dir,
                project_settings.packages.clone().unwrap_or_default(),
            ),
        ]
        .concat();
        let package_theme_paths = if cli.no_themes {
            Vec::new()
        } else {
            package_theme_paths(&packages)?
        };
        let settings_prompt_paths = if cli.no_prompt_templates {
            Vec::new()
        } else {
            [
                resource_path_specs(
                    &user_app_dir,
                    global_settings.prompts.clone().unwrap_or_default(),
                ),
                resource_path_specs(
                    &app_dir,
                    project_settings.prompts.clone().unwrap_or_default(),
                ),
            ]
            .concat()
        };
        let prompt_paths = [
            settings_prompt_paths,
            resource_path_specs(&cwd, pathbuf_values(cli.prompt_templates.clone())),
        ]
        .concat();
        let settings_skill_paths = if cli.no_skills {
            Vec::new()
        } else {
            [
                resource_path_specs(
                    &user_app_dir,
                    global_settings
                        .skills
                        .clone()
                        .map(SkillsSetting::into_paths)
                        .unwrap_or_default(),
                ),
                resource_path_specs(
                    &app_dir,
                    project_settings
                        .skills
                        .clone()
                        .map(SkillsSetting::into_paths)
                        .unwrap_or_default(),
                ),
            ]
            .concat()
        };
        let skill_paths = [
            settings_skill_paths,
            resource_path_specs(&cwd, pathbuf_values(cli.skills.clone())),
        ]
        .concat();
        let settings_theme_paths = if cli.no_themes {
            Vec::new()
        } else {
            [
                resolve_config_paths(
                    &user_app_dir,
                    global_settings.themes.clone().unwrap_or_default(),
                ),
                resolve_config_paths(
                    &app_dir,
                    project_settings.themes.clone().unwrap_or_default(),
                ),
            ]
            .concat()
        };
        let theme_paths = [
            settings_theme_paths,
            package_theme_paths,
            resolve_cli_paths(&cwd, cli.themes.clone()),
        ]
        .concat();
        let system_prompt = if let Some(value) = cli.system_prompt.clone() {
            Some(resolve_prompt_input(&cwd, value)?)
        } else if let Some(value) = project_settings.system_prompt {
            Some(resolve_prompt_input(&app_dir, value)?)
        } else if let Some(value) = global_settings.system_prompt {
            Some(resolve_prompt_input(&user_app_dir, value)?)
        } else if let Some(path) =
            discover_system_prompt_file(&app_dir, &user_app_dir, project_trusted)
        {
            Some(
                fs::read_to_string(&path)
                    .with_context(|| format!("failed to read system prompt {}", path.display()))?,
            )
        } else {
            None
        };
        let shell_path = project_settings.shell_path.or(global_settings.shell_path);
        let shell_command_prefix = project_settings
            .shell_command_prefix
            .or(global_settings.shell_command_prefix);
        let npm_command = project_settings
            .npm_command
            .or(global_settings.npm_command)
            .filter(|command| !command.is_empty());
        let enable_skill_commands = project_settings
            .enable_skill_commands
            .or_else(|| {
                project_settings
                    .skills
                    .as_ref()
                    .and_then(SkillsSetting::enable_skill_commands)
            })
            .or(global_settings.enable_skill_commands)
            .or_else(|| {
                global_settings
                    .skills
                    .as_ref()
                    .and_then(SkillsSetting::enable_skill_commands)
            })
            .unwrap_or(true);
        let context_files = if cli.no_context_files {
            Vec::new()
        } else {
            load_context_files(&cwd, &user_app_dir)?
        };
        let auth_paths = vec![user_app_dir.join("auth.json")];

        let compaction = project_settings
            .compaction
            .clone()
            .or_else(|| global_settings.compaction.clone());
        let retry = project_settings
            .retry
            .clone()
            .or_else(|| global_settings.retry.clone());
        let compaction_enabled = compaction.as_ref().and_then(|c| c.enabled).unwrap_or(true);
        let compaction_reserve_tokens = compaction
            .as_ref()
            .and_then(|c| c.reserve_tokens)
            .unwrap_or(DEFAULT_COMPACTION_RESERVE_TOKENS);
        let compaction_keep_recent_tokens = compaction
            .as_ref()
            .and_then(|c| c.keep_recent_tokens)
            .unwrap_or(DEFAULT_COMPACTION_KEEP_RECENT_TOKENS);
        let retry_max_retries = retry
            .as_ref()
            .and_then(|r| r.max_retries)
            .unwrap_or(DEFAULT_RETRY_MAX_RETRIES);
        // Stream by default: tokens arrive live AND a long turn can't hit the
        // "operation timed out" you get waiting for a whole non-streamed body
        // (it cut off mid-work). Explicit settings still override.
        let stream = cli.stream
            || project_settings
                .stream
                .or(global_settings.stream)
                .unwrap_or(true);
        let review_model = project_settings
            .review_model
            .clone()
            .or_else(|| global_settings.review_model.clone());

        Ok(Self {
            cwd,
            app_dir,
            user_app_dir,
            project_trusted,
            project_resources_detected,
            session_dir,
            provider: if cli.provider != "openai-codex" {
                cli.provider.clone()
            } else {
                project_settings
                    .default_provider
                    .or(global_settings.default_provider)
                    .unwrap_or_else(|| cli.provider.clone())
            },
            model: cli
                .model
                .clone()
                .or(project_settings.default_model)
                .or(global_settings.default_model),
            thinking_level,
            default_persona,
            api_key: cli.api_key.clone(),
            system_prompt,
            append_system_prompt,
            context_files,
            favorites,
            no_tools: cli.no_tools,
            no_builtin_tools: cli.no_builtin_tools,
            tool_allowlist,
            tool_exclude,
            no_extensions: cli.no_extensions,
            no_skills: cli.no_skills,
            no_prompt_templates: cli.no_prompt_templates,
            no_context_files: cli.no_context_files,
            no_themes: cli.no_themes,
            extension_paths,
            packages,
            prompt_paths,
            skill_paths,
            theme_paths,
            enable_skill_commands,
            shell_path,
            shell_command_prefix,
            npm_command,
            auth_paths,
            compaction_enabled,
            compaction_reserve_tokens,
            compaction_keep_recent_tokens,
            retry_max_retries,
            stream,
            review_model,
        })
    }

    pub fn models_json_paths(&self) -> Vec<PathBuf> {
        vec![
            self.app_dir.join("models.json"),
            self.user_app_dir.join("models.json"),
            self.models_dev_cache_path(),
        ]
    }

    /// Auto-managed catalog written by `/models refresh` (kept separate from any
    /// hand-written models.json so a refresh never clobbers user edits).
    pub fn models_dev_cache_path(&self) -> PathBuf {
        self.user_app_dir.join("models-dev.json")
    }

    /// Legacy per-project standing goal file, kept for migration/clear only.
    pub fn goal_file(&self) -> PathBuf {
        let canonical =
            fs::canonicalize(&self.cwd).unwrap_or_else(|_| normalize_lexical(self.cwd.clone()));
        let raw = canonical.display().to_string();
        let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let label = self
            .cwd
            .file_name()
            .map(|name| name.to_string_lossy())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "project".into())
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .take(48)
            .collect::<String>();
        let label = label.trim_matches('-');
        let label = if label.is_empty() { "project" } else { label };
        let key = format!("{label}-{}", &hash[..16]);
        self.user_app_dir.join("goals").join(format!("{key}.md"))
    }
}

/// Resolve a configured secret-ish value (API keys, tokens):
/// - `!command` — run the command (10s cap), use trimmed stdout; cached per
///   process so a `pass`/`op`/`gcloud` helper runs once. Failure → None.
/// - a SCREAMING_SNAKE name that matches a set environment variable — use the
///   env value, so config files can reference keys without embedding them.
/// - anything else — the literal value.
pub(crate) fn resolve_config_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(command) = trimmed.strip_prefix('!') {
        static CACHE: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
        > = std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        if let Ok(guard) = cache.lock()
            && let Some(cached) = guard.get(command)
        {
            return cached.clone();
        }
        let resolved = crate::tools::run_config_command(command);
        if let Ok(mut guard) = cache.lock() {
            guard.insert(command.to_string(), resolved.clone());
        }
        return resolved;
    }
    let looks_like_env_name = trimmed.len() >= 3
        && trimmed
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && trimmed.chars().any(|c| c.is_ascii_uppercase());
    if looks_like_env_name
        && let Ok(env_value) = std::env::var(trimmed)
        && !env_value.trim().is_empty()
    {
        return Some(env_value.trim().to_string());
    }
    Some(trimmed.to_string())
}

/// Cross-process lock for shared state files (settings.json, trust.json):
/// atomic `mkdir` of `<file>.lock` with a `{pid, ts}` owner token. A stale
/// lock is reclaimed only when its owner PID is provably dead, or after 10s
/// when liveness cannot be determined — a slow-but-alive owner is never
/// robbed. The owner token is re-compared immediately before removal so two
/// reapers cannot race each other into deleting a freshly re-acquired lock.
pub(crate) struct FileLockGuard {
    lock_dir: PathBuf,
    token: String,
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        let owner = self.lock_dir.join("owner.json");
        if fs::read_to_string(&owner)
            .map(|text| text == self.token)
            .unwrap_or(false)
        {
            let _ = fs::remove_file(&owner);
            let _ = fs::remove_dir(&self.lock_dir);
        }
    }
}

fn process_alive(pid: u32) -> Option<bool> {
    #[cfg(windows)]
    {
        let output = crate::spawn::no_window_command("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")))
    }
    #[cfg(unix)]
    {
        let status = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .ok()?;
        Some(status.success())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        None
    }
}

/// Acquire the lock for `target`, waiting up to ~5s. Returns None when the
/// lock could not be won — callers proceed unlocked (best-effort) rather than
/// failing the user's operation.
pub(crate) fn lock_state_file(target: &Path) -> Option<FileLockGuard> {
    let name = target.file_name()?.to_string_lossy().into_owned();
    let lock_dir = target.with_file_name(format!("{name}.lock"));
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let token = format!("{{\"pid\":{},\"ts\":{now_ms}}}", std::process::id());
    for _ in 0..50 {
        match fs::create_dir(&lock_dir) {
            Ok(()) => {
                let _ = fs::write(lock_dir.join("owner.json"), &token);
                return Some(FileLockGuard { lock_dir, token });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let owner_path = lock_dir.join("owner.json");
                let observed = fs::read_to_string(&owner_path).ok();
                let reclaimable = match observed
                    .as_deref()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                {
                    Some(owner) => {
                        let pid = owner.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let ts = owner.get("ts").and_then(|v| v.as_u64()).unwrap_or(0) as u128;
                        match process_alive(pid) {
                            Some(false) => true,
                            Some(true) => false,
                            None => now_ms.saturating_sub(ts) > 10_000,
                        }
                    }
                    // No/invalid owner token: creator crashed between mkdir
                    // and write, or hand-made junk — reclaim.
                    None => true,
                };
                if reclaimable {
                    // Compare-token guard: only remove what we actually judged.
                    if fs::read_to_string(&owner_path).ok() == observed {
                        let _ = fs::remove_file(&owner_path);
                        let _ = fs::remove_dir(&lock_dir);
                    }
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
    None
}

fn read_settings(path: PathBuf) -> Result<Settings> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let text = text.trim_start_matches('\u{feff}');
    match serde_json::from_str(text) {
        Ok(settings) => Ok(settings),
        Err(error) => {
            // Corrupt is NOT absent: a syntax error must never brick startup
            // (the old `?` here did) or silently reset the user's settings.
            // Keep the evidence as a backup, warn, and run this layer on
            // defaults — the other layers still apply.
            let backup = path.with_extension("json.corrupt.bak");
            let _ = fs::copy(&path, &backup);
            eprintln!(
                "bbarit: {} is invalid JSON ({error}). Continuing with defaults for this \
                 layer; the broken file was backed up to {} — fix it or delete it.",
                path.display(),
                backup.display()
            );
            Ok(Settings::default())
        }
    }
}

fn read_merged_settings(paths: impl IntoIterator<Item = PathBuf>) -> Result<Settings> {
    let mut merged = Settings::default();
    for path in paths {
        let next = read_settings(path)?;
        merge_settings(&mut merged, next);
    }
    Ok(merged)
}

fn merge_settings(base: &mut Settings, next: Settings) {
    if next.default_provider.is_some() {
        base.default_provider = next.default_provider;
    }
    if next.default_model.is_some() {
        base.default_model = next.default_model;
    }
    if next.default_persona.is_some() {
        base.default_persona = next.default_persona;
    }
    if next.default_thinking_level.is_some() {
        base.default_thinking_level = next.default_thinking_level;
    }
    if next.enabled_models.is_some() {
        base.enabled_models = next.enabled_models;
    }
    if next.session_dir.is_some() {
        base.session_dir = next.session_dir;
    }
    if next.system_prompt.is_some() {
        base.system_prompt = next.system_prompt;
    }
    if next.append_system_prompt.is_some() {
        base.append_system_prompt = next.append_system_prompt;
    }
    if next.packages.is_some() {
        base.packages = next.packages;
    }
    if next.extensions.is_some() {
        base.extensions = next.extensions;
    }
    if next.prompts.is_some() {
        base.prompts = next.prompts;
    }
    if next.skills.is_some() {
        base.skills = next.skills;
    }
    if next.themes.is_some() {
        base.themes = next.themes;
    }
    if next.enable_skill_commands.is_some() {
        base.enable_skill_commands = next.enable_skill_commands;
    }
    if next.shell_path.is_some() {
        base.shell_path = next.shell_path;
    }
    if next.shell_command_prefix.is_some() {
        base.shell_command_prefix = next.shell_command_prefix;
    }
    if next.npm_command.is_some() {
        base.npm_command = next.npm_command;
    }
    if next.default_project_trust.is_some() {
        base.default_project_trust = next.default_project_trust;
    }
    if next.compaction.is_some() {
        base.compaction = next.compaction;
    }
    if next.retry.is_some() {
        base.retry = next.retry;
    }
    if next.stream.is_some() {
        base.stream = next.stream;
    }
    if next.review_model.is_some() {
        base.review_model = next.review_model;
    }
}

fn resolve_config_path(cwd: &Path, value: String) -> PathBuf {
    let path = resolve_home_path(value);
    let resolved = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    normalize_lexical(resolved)
}

fn resolve_config_paths(base_dir: &Path, values: Vec<String>) -> Vec<PathBuf> {
    values
        .into_iter()
        .map(|path| resolve_config_path(base_dir, path))
        .collect()
}

fn resolve_cli_paths(cwd: &Path, values: Vec<PathBuf>) -> Vec<PathBuf> {
    values
        .into_iter()
        .map(|path| resolve_config_path(cwd, path.display().to_string()))
        .collect()
}

fn pathbuf_values(values: Vec<PathBuf>) -> Vec<String> {
    values
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn resource_path_specs(base_dir: &Path, values: Vec<String>) -> Vec<ResourcePathSpec> {
    values
        .into_iter()
        .map(|value| ResourcePathSpec {
            base_dir: base_dir.to_path_buf(),
            value,
        })
        .collect()
}

fn package_specs(base_dir: &Path, values: Vec<PackageSetting>) -> Vec<PackageSpec> {
    values
        .into_iter()
        .map(|value| match value {
            PackageSetting::Source(source) => PackageSpec {
                base_dir: base_dir.to_path_buf(),
                source,
                extensions: None,
                skills: None,
                prompts: None,
                themes: None,
            },
            PackageSetting::Filtered(filter) => PackageSpec {
                base_dir: base_dir.to_path_buf(),
                source: filter.source,
                extensions: filter.extensions.map(StringList::into_vec),
                skills: filter.skills.map(StringList::into_vec),
                prompts: filter.prompts.map(StringList::into_vec),
                themes: filter.themes.map(StringList::into_vec),
            },
        })
        .collect()
}

#[derive(Debug, Deserialize, Default)]
struct PackageThemeJson {
    pi: Option<PackageThemeManifest>,
}

#[derive(Debug, Deserialize, Default)]
struct PackageThemeManifest {
    themes: Option<Vec<String>>,
}

fn package_theme_paths(packages: &[PackageSpec]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for package in packages {
        append_package_theme_paths(package, &mut paths)?;
    }
    Ok(paths)
}

fn append_package_theme_paths(package: &PackageSpec, out: &mut Vec<PathBuf>) -> Result<()> {
    let root = package.resolved_root();
    if !root.exists() {
        return Ok(());
    }
    if let Some(filter) = package.themes.as_ref() {
        out.extend(
            filter
                .iter()
                .map(|entry| resolve_package_entry(&root, entry)),
        );
        return Ok(());
    }
    if let Some(entries) = package_theme_manifest_entries(&root)? {
        out.extend(
            entries
                .iter()
                .map(|entry| resolve_package_entry(&root, entry)),
        );
    } else {
        out.push(root.join("themes"));
    }
    Ok(())
}

fn package_theme_manifest_entries(root: &Path) -> Result<Option<Vec<String>>> {
    let manifest_path = root.join("package.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let package: PackageThemeJson = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    Ok(package.pi.and_then(|manifest| manifest.themes))
}

fn resolve_package_entry(root: &Path, entry: &str) -> PathBuf {
    let path = resolve_home_path(entry.to_string());
    if path.is_absolute() {
        normalize_lexical(path)
    } else {
        normalize_lexical(root.join(path))
    }
}

pub fn is_local_package_source(source: &str) -> bool {
    let value = source.trim();
    !is_npm_package_source(value) && !is_git_package_source(value)
}

pub fn is_npm_package_source(source: &str) -> bool {
    source.trim().starts_with("npm:")
}

pub fn is_git_package_source(source: &str) -> bool {
    let value = source.trim();
    value.starts_with("git:")
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("ssh://")
        || value.starts_with("git://")
}

pub fn resolved_package_root(base_dir: &Path, source: &str) -> PathBuf {
    let source = source.trim();
    if is_npm_package_source(source) {
        return package_storage_base(base_dir)
            .join("npm")
            .join("node_modules")
            .join(npm_package_name(
                source.strip_prefix("npm:").unwrap_or(source),
            ));
    }
    if is_git_package_source(source) {
        return package_storage_base(base_dir)
            .join("git")
            .join(safe_package_dir_name(source));
    }
    resolve_config_path(base_dir, source.to_string())
}

pub fn package_storage_base(base_dir: &Path) -> PathBuf {
    env::var(PI_PACKAGE_DIR_ENV)
        .ok()
        .map(resolve_home_path)
        .unwrap_or_else(|| base_dir.to_path_buf())
}

pub fn npm_package_name(spec: &str) -> PathBuf {
    let spec = spec.trim();
    let without_file = spec
        .strip_prefix("file:")
        .or_else(|| spec.strip_prefix("link:"))
        .unwrap_or(spec);
    let name = if without_file.starts_with('@') {
        let mut parts = without_file.split('@');
        let _ = parts.next();
        let scoped_name = parts.next().unwrap_or(without_file);
        format!("@{scoped_name}")
    } else {
        without_file
            .split('@')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(without_file)
            .to_string()
    };
    name.split('/').fold(PathBuf::new(), |mut path, part| {
        path.push(part);
        path
    })
}

pub fn safe_package_dir_name(source: &str) -> String {
    let mut out = String::new();
    for ch in source.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let out = out.trim_matches('_');
    if out.is_empty() {
        "package".to_string()
    } else {
        out.chars().take(120).collect()
    }
}

fn split_resource_pattern_prefix(value: &str) -> (&str, &str) {
    if let Some(rest) = value.strip_prefix('!') {
        ("!", rest)
    } else if let Some(rest) = value.strip_prefix('+') {
        ("+", rest)
    } else if let Some(rest) = value.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", value)
    }
}

fn parse_tool_list(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn resolve_pathbuf_home(path: PathBuf) -> PathBuf {
    if let Some(value) = path.to_str() {
        resolve_home_path(value.to_string())
    } else {
        path
    }
}

/// Path to the user-level settings.json (honors PI_CODING_AGENT_DIR).
pub fn user_settings_file() -> PathBuf {
    let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
    env::var(PI_AGENT_DIR_ENV)
        .ok()
        .map(resolve_home_path)
        .unwrap_or_else(|| home.join(USER_APP_ROOT).join("agent"))
        .join("settings.json")
}

/// Persist the chosen provider/model as the launch default (sticky model),
/// merging into the existing user settings.json. Best-effort.
pub fn persist_default_model(provider: &str, model_id: &str) -> Result<()> {
    let path = user_settings_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Lock + re-read + patch only our keys + atomic write: concurrent bbarit
    // processes (TUI, panel, subagents) each preserve the others' settings.
    let _lock = lock_state_file(&path);
    let raw = fs::read_to_string(&path).ok();
    let parsed = raw
        .as_deref()
        .map(|text| serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')));
    // Never silently reset a corrupt file to {} — that destroys every other
    // setting in it. Back it up first so the user can recover.
    if raw.is_some() && !matches!(parsed, Some(Ok(_))) {
        let _ = fs::copy(&path, path.with_extension("json.corrupt.bak"));
    }
    let mut value = match parsed {
        Some(Ok(value)) if value.is_object() => value,
        _ => serde_json::json!({}),
    };
    value["defaultProvider"] = serde_json::json!(provider);
    value["defaultModel"] = serde_json::json!(model_id);
    // Atomic: a crash mid-write must not corrupt settings.json.
    crate::tools::atomic_write(&path, serde_json::to_string_pretty(&value)?.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn resolve_home_path(value: String) -> PathBuf {
    if value == "~" {
        return dirs_next::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }

    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
        && let Some(home) = dirs_next::home_dir()
    {
        return join_path_components(home, rest);
    }

    PathBuf::from(value)
}

fn join_path_components(mut base: PathBuf, rest: &str) -> PathBuf {
    for part in rest.split(['/', '\\']).filter(|part| !part.is_empty()) {
        base.push(part);
    }
    base
}

fn normalize_lexical(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn resolve_project_trusted(
    cli: &Cli,
    cwd: &Path,
    user_app_dir: &Path,
    project_resources_detected: bool,
    default_project_trust: Option<&str>,
) -> Result<bool> {
    if !project_resources_detected {
        return Ok(true);
    }
    if cli.approve {
        return Ok(true);
    }
    if cli.no_approve {
        return Ok(false);
    }
    if let Some(decision) = read_project_trust_decision(user_app_dir, cwd)? {
        return Ok(decision);
    }
    // Trust by default: asking on every new folder (or silently starting
    // untrusted) was pure friction. An explicit /trust no still blocks a
    // folder, and default_project_trust="never" restores ask-nothing-load-nothing.
    Ok(!matches!(default_project_trust, Some("never")))
}

fn read_project_trust_decision(user_app_dir: &Path, cwd: &Path) -> Result<Option<bool>> {
    let path = user_app_dir.join("trust.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let raw = raw.trim_start_matches('\u{feff}');
    let data: serde_json::Value = match serde_json::from_str(raw) {
        Ok(data) => data,
        Err(error) => {
            // Same corrupt-is-not-absent policy as settings: back up, warn,
            // and fall back to "no recorded answer" (the trust prompt runs
            // again) instead of refusing to start.
            let backup = path.with_extension("json.corrupt.bak");
            let _ = fs::copy(&path, &backup);
            eprintln!(
                "bbarit: {} is invalid JSON ({error}); backed up to {} and ignoring it.",
                path.display(),
                backup.display()
            );
            return Ok(None);
        }
    };
    let Some(object) = data.as_object() else {
        return Ok(None);
    };
    let mut current = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    loop {
        let key = current.display().to_string();
        if let Some(value) = object.get(&key).and_then(|value| value.as_bool()) {
            return Ok(Some(value));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn has_project_resources(cwd: &Path, app_dir: &Path) -> bool {
    [app_dir].iter().any(|pi_dir| {
        [
            "settings.json",
            "extensions",
            "skills",
            "prompts",
            "themes",
            "SYSTEM.md",
            "APPEND_SYSTEM.md",
        ]
        .iter()
        .any(|entry| pi_dir.join(entry).exists())
    }) || ancestor_agents_skills(cwd)
        .into_iter()
        .any(|path| path.exists())
}

fn ancestor_agents_skills(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = Some(cwd);
    while let Some(dir) = current {
        dirs.push(dir.join(".agents").join("skills"));
        current = dir.parent();
    }
    dirs
}

fn load_context_files(cwd: &Path, user_app_dir: &Path) -> Result<Vec<ContextFile>> {
    let mut files = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    collect_context_file(user_app_dir, &mut seen, &mut files)?;

    let mut ancestors = Vec::new();
    let mut current = Some(cwd);
    while let Some(dir) = current {
        ancestors.push(dir.to_path_buf());
        current = dir.parent();
    }
    ancestors.reverse();
    for dir in ancestors {
        collect_context_file(&dir, &mut seen, &mut files)?;
    }
    Ok(files)
}

// Context files ride along on EVERY request, so an oversized AGENTS/CLAUDE.md
// silently multiplies token spend. Inject at most this many bytes per file and
// tell the model where the rest lives. BBARIT_CONTEXT_FILE_LIMIT overrides
// (0 = unlimited).
const CONTEXT_FILE_PROMPT_LIMIT: usize = 20_000;

fn context_file_limit() -> usize {
    env::var("BBARIT_CONTEXT_FILE_LIMIT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(CONTEXT_FILE_PROMPT_LIMIT)
}

pub fn truncate_context_for_prompt(content: String, path: &Path, limit: usize) -> String {
    if limit == 0 || content.len() <= limit {
        return content;
    }
    let mut cut = limit;
    while !content.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}\n\n[context file truncated at {limit} bytes — use the read tool on {} for the rest]",
        &content[..cut],
        path.display()
    )
}

fn collect_context_file(
    dir: &Path,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    out: &mut Vec<ContextFile>,
) -> Result<()> {
    for name in [
        "AGENTS.md",
        "AGENTS.MD",
        "AGENT.md",
        "AGENT.MD",
        "CLAUDE.md",
        "CLAUDE.MD",
    ] {
        let path = dir.join(name);
        if path.exists() && seen.insert(path.clone()) {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let content = truncate_context_for_prompt(content, &path, context_file_limit());
            out.push(ContextFile { path, content });
            break;
        }
    }
    Ok(())
}

fn resolve_prompt_input(cwd: &Path, value: String) -> Result<String> {
    if value.trim().is_empty() {
        return Ok(value);
    }
    let path = PathBuf::from(&value);
    let candidate = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    if candidate.exists() && candidate.is_file() {
        fs::read_to_string(&candidate)
            .with_context(|| format!("failed to read prompt file {}", candidate.display()))
    } else {
        Ok(value)
    }
}

fn discover_system_prompt_file(
    app_dir: &Path,
    user_app_dir: &Path,
    project_trusted: bool,
) -> Option<PathBuf> {
    let project = app_dir.join("SYSTEM.md");
    if project_trusted && project.exists() {
        return Some(project);
    }
    let global = user_app_dir.join("SYSTEM.md");
    global.exists().then_some(global)
}

fn discover_append_system_prompt_file(
    app_dir: &Path,
    user_app_dir: &Path,
    project_trusted: bool,
) -> Option<PathBuf> {
    let project = app_dir.join("APPEND_SYSTEM.md");
    if project_trusted && project.exists() {
        return Some(project);
    }
    let global = user_app_dir.join("APPEND_SYSTEM.md");
    global.exists().then_some(global)
}

/// The agent's own dotenv (~/.bbarit-oss/agent/.env) — API keys and feature toggles.
pub fn agent_env_path() -> Option<PathBuf> {
    dirs_next::home_dir().map(|home| home.join(USER_APP_ROOT).join("agent").join(".env"))
}

/// Read a single variable, process env first, then the agent dotenv.
pub fn agent_env_var(name: &str) -> Option<String> {
    if let Ok(value) = env::var(name) {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let content = fs::read_to_string(agent_env_path()?).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(name)
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Replace/add one variable line in the agent dotenv; remove it when None.
/// Other lines are preserved as-is.
pub fn set_agent_env_var(name: &str, value: Option<&str>) -> Result<()> {
    let path = agent_env_path().ok_or_else(|| anyhow!("cannot resolve the home directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = content
        .lines()
        .filter(|line| !line.trim_start().starts_with(&format!("{name}=")))
        .map(str::to_string)
        .collect();
    if let Some(value) = value {
        lines.push(format!("{name}={value}"));
    }
    let mut out = lines.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    fs::write(&path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_paths_expand_tilde_to_home() {
        let home = dirs_next::home_dir().expect("home directory is available");
        assert_eq!(resolve_home_path("~".to_string()), home);
    }

    #[test]
    fn config_values_resolve_env_first_with_command_escape() {
        // Literal values pass through untouched.
        assert_eq!(
            resolve_config_value("sk-abc123").as_deref(),
            Some("sk-abc123")
        );
        // A SET env-var name resolves to the env value.
        unsafe { std::env::set_var("BBARIT_TEST_SECRET_XYZ", "from-env") };
        assert_eq!(
            resolve_config_value("BBARIT_TEST_SECRET_XYZ").as_deref(),
            Some("from-env")
        );
        unsafe { std::env::remove_var("BBARIT_TEST_SECRET_XYZ") };
        // An UNSET env-shaped name falls back to the literal.
        assert_eq!(
            resolve_config_value("BBARIT_TEST_UNSET_VAR_QQ").as_deref(),
            Some("BBARIT_TEST_UNSET_VAR_QQ")
        );
        // `!command` runs the helper and uses stdout (also exercises the cache).
        assert_eq!(
            resolve_config_value("!echo helper-secret").as_deref(),
            Some("helper-secret")
        );
        assert_eq!(
            resolve_config_value("!echo helper-secret").as_deref(),
            Some("helper-secret")
        );
        // Empty resolves to None.
        assert_eq!(resolve_config_value("  "), None);
    }

    #[test]
    fn goal_file_uses_bounded_hash_key_to_avoid_path_collisions() {
        let root = std::env::temp_dir().join("bbarit-goal-file-test");
        let mut first = AppConfig::for_test(PathBuf::from("/tmp/a-b"));
        let mut second = AppConfig::for_test(PathBuf::from("/tmp/a/b"));
        first.user_app_dir = root.clone();
        second.user_app_dir = root;

        let first_goal = first.goal_file();
        let second_goal = second.goal_file();

        assert_ne!(first_goal, second_goal);
        assert_eq!(
            first_goal.parent().and_then(Path::file_name).unwrap(),
            "goals"
        );
        assert!(
            first_goal.file_name().unwrap().to_string_lossy().len() < 80,
            "goal file name should stay short: {}",
            first_goal.display()
        );
    }

    #[test]
    fn state_file_lock_acquires_releases_and_reclaims_dead_owner() {
        let dir = std::env::temp_dir().join("bbarit-config-lock");
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("settings.json");

        // Acquire → drop → reacquire works.
        let guard = lock_state_file(&target).expect("first acquire");
        let lock_dir = dir.join("settings.json.lock");
        assert!(lock_dir.exists());
        drop(guard);
        assert!(!lock_dir.exists(), "drop must release the lock");

        // A lock held by a dead PID is reclaimed instead of waited out.
        fs::create_dir(&lock_dir).unwrap();
        fs::write(lock_dir.join("owner.json"), "{\"pid\":999999999,\"ts\":0}").unwrap();
        let reclaimed = lock_state_file(&target).expect("dead owner must be reclaimed");
        drop(reclaimed);
        assert!(!lock_dir.exists());
    }

    #[test]
    fn corrupt_settings_backs_up_and_falls_back_to_defaults() {
        // Corrupt is NOT absent: startup must survive, the broken file must be
        // preserved as evidence, and the layer runs on defaults.
        let dir = std::env::temp_dir().join("bbarit-config-corrupt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        fs::write(&path, "{ this is not json").unwrap();
        let backup = path.with_extension("json.corrupt.bak");
        let _ = fs::remove_file(&backup);
        let settings = read_settings(path.clone()).expect("corrupt settings must not error");
        assert!(settings.default_provider.is_none());
        assert!(backup.exists(), "corrupt file must be backed up");
    }

    #[test]
    fn config_paths_expand_tilde_children_before_cwd_resolution() {
        let home = dirs_next::home_dir().expect("home directory is available");
        let cwd = env::current_dir().expect("cwd is available");

        assert_eq!(
            resolve_config_path(&cwd, "~/.pi/agent/prompts".to_string()),
            home.join(".pi").join("agent").join("prompts")
        );
        assert_eq!(
            resolve_config_path(&cwd, "~\\.pi\\agent\\skills".to_string()),
            home.join(".pi").join("agent").join("skills")
        );
    }

    #[test]
    fn config_paths_keep_relative_values_project_scoped() {
        let cwd = env::current_dir().expect("cwd is available");
        assert_eq!(
            resolve_config_path(&cwd, "resources/prompts".to_string()),
            cwd.join("resources/prompts")
        );
        assert_eq!(
            resolve_config_path(&cwd.join(".pi"), "../pkg".to_string()),
            cwd.join("pkg")
        );
    }

    #[test]
    fn config_path_lists_resolve_against_their_scope_base() {
        let base = env::current_dir().expect("cwd is available").join(".pi");
        assert_eq!(
            resolve_config_paths(
                &base,
                vec![
                    "prompts".to_string(),
                    "skills/review".to_string(),
                    "~/.pi/agent/shared".to_string(),
                ],
            ),
            vec![
                base.join("prompts"),
                base.join("skills").join("review"),
                dirs_next::home_dir()
                    .expect("home directory is available")
                    .join(".pi")
                    .join("agent")
                    .join("shared"),
            ]
        );
    }

    #[test]
    fn context_files_truncate_at_limit_with_read_pointer() {
        let path = Path::new("/proj/CLAUDE.md");
        // Under the limit: untouched.
        let small = "short context".to_string();
        assert_eq!(truncate_context_for_prompt(small.clone(), path, 100), small);
        // Over the limit: clamped, and the model is told where the rest lives.
        let big = "x".repeat(300);
        let out = truncate_context_for_prompt(big, path, 100);
        assert!(out.starts_with(&"x".repeat(100)));
        assert!(out.contains("truncated at 100 bytes"));
        assert!(out.contains("/proj/CLAUDE.md"));
        // Multibyte boundary: never splits a char.
        let korean = "한글컨텍스트".repeat(50).to_string();
        let out = truncate_context_for_prompt(korean, path, 100);
        assert!(out.contains("truncated"));
        // Limit 0 = unlimited.
        let big = "y".repeat(300);
        assert_eq!(truncate_context_for_prompt(big.clone(), path, 0), big);
    }
}
