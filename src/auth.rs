use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::AppConfig;

const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_CODEX_AUTH_BASE: &str = "https://auth.openai.com";
const OPENAI_CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CODEX_BROWSER_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_CODEX_DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const OPENAI_CODEX_SCOPE: &str = "openid profile email offline_access";
const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ANTHROPIC_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const ANTHROPIC_CALLBACK_PORT: u16 = 53692;
const ANTHROPIC_CALLBACK_PATH: &str = "/callback";
const ANTHROPIC_REDIRECT_URI: &str = "http://localhost:53692/callback";
const ANTHROPIC_SCOPE: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StoredCredential {
    ApiKey {
        key: Option<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Oauth {
        access: String,
        refresh: Option<String>,
        expires: Option<i64>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
}

/// Providers that may hold several logged-in accounts at once. The active
/// account lives at the plain provider key (so every existing lookup keeps
/// working); extra accounts live at `<provider>#2`, `<provider>#3`, …
pub const MULTI_ACCOUNT_PROVIDERS: &[&str] = &["anthropic", "openai-codex"];

pub fn is_multi_account_provider(provider_id: &str) -> bool {
    MULTI_ACCOUNT_PROVIDERS.contains(&provider_id)
}

/// One stored login, as shown in `/accounts`.
#[derive(Debug, Clone)]
pub struct AccountInfo {
    /// Auth-store key: `anthropic` (active) or `anthropic#2` (stored).
    pub key: String,
    pub provider_id: String,
    /// Email when known, else a short account id, else a generic label.
    pub label: String,
    pub active: bool,
    pub oauth: bool,
}

/// Provider part of an account key (`anthropic#2` → `anthropic`).
pub fn account_provider(key: &str) -> &str {
    key.split_once('#')
        .map(|(provider, _)| provider)
        .unwrap_or(key)
}

fn family_key_matches(provider: &str, key: &str) -> bool {
    key == provider
        || key
            .strip_prefix(provider)
            .and_then(|tail| tail.strip_prefix('#'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn slot_number(provider: &str, key: &str) -> Option<u32> {
    key.strip_prefix(provider)?.strip_prefix('#')?.parse().ok()
}

/// All stored accounts for a provider: the active one first, then extra slots
/// in numeric order. The first auth path wins for duplicate keys, matching
/// `stored_api_key` precedence.
fn collect_accounts(config: &AppConfig, provider: &str) -> Result<Vec<(String, StoredCredential)>> {
    let mut seen: BTreeMap<String, StoredCredential> = BTreeMap::new();
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        for (key, credential) in read_auth(path)? {
            if family_key_matches(provider, &key) && !seen.contains_key(&key) {
                seen.insert(key, credential);
            }
        }
    }
    let mut ordered = Vec::new();
    if let Some(active) = seen.remove(provider) {
        ordered.push((provider.to_string(), active));
    }
    let mut slots: Vec<(u32, String, StoredCredential)> = seen
        .into_iter()
        .filter_map(|(key, credential)| slot_number(provider, &key).map(|n| (n, key, credential)))
        .collect();
    slots.sort_by_key(|(n, ..)| *n);
    ordered.extend(
        slots
            .into_iter()
            .map(|(_, key, credential)| (key, credential)),
    );
    Ok(ordered)
}

/// Rewrite a provider's whole account family: entry 0 becomes the active
/// plain key, the rest become `#2`, `#3`, … All family keys are first removed
/// from every auth store so a stale legacy file can't shadow the new state.
/// True if the user already has at least one usable credential — a stored login
/// in auth.json, or a well-known provider API-key env var. A fresh run with none
/// opens the login picker automatically so the first thing a new user sees is
/// "how do I sign in?" instead of an auth error on their first message.
pub fn has_any_login(config: &AppConfig) -> bool {
    for path in &config.auth_paths {
        if path.exists()
            && read_auth(path)
                .map(|creds| !creds.is_empty())
                .unwrap_or(false)
        {
            return true;
        }
    }
    const COMMON_KEY_ENVS: &[&str] = &[
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "OPENROUTER_API_KEY",
        "GROQ_API_KEY",
        "XAI_API_KEY",
        "DEEPSEEK_API_KEY",
        "MISTRAL_API_KEY",
        "DASHSCOPE_API_KEY",
        "ZAI_API_KEY",
        "MOONSHOT_API_KEY",
    ];
    COMMON_KEY_ENVS
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|value| !value.trim().is_empty()))
}

fn write_family(config: &AppConfig, provider: &str, ordered: &[StoredCredential]) -> Result<()> {
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let mut data = read_auth_values(path)?;
        let before = data.len();
        data.retain(|key, _| !family_key_matches(provider, key));
        if data.len() != before {
            write_auth_values(path, &data)?;
        }
    }
    let path = primary_auth_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut data = read_auth_values(&path)?;
    for (index, credential) in ordered.iter().enumerate() {
        let key = if index == 0 {
            provider.to_string()
        } else {
            format!("{provider}#{}", index + 1)
        };
        data.insert(key, serde_json::to_value(credential)?);
    }
    write_auth_values(&path, &data)
}

pub fn list_accounts(config: &AppConfig, provider: &str) -> Result<Vec<AccountInfo>> {
    Ok(collect_accounts(config, provider)?
        .into_iter()
        .map(|(key, credential)| AccountInfo {
            active: key == provider,
            provider_id: provider.to_string(),
            label: credential_label(&credential),
            oauth: matches!(credential, StoredCredential::Oauth { .. }),
            key,
        })
        .collect())
}

/// Email of the active login for a provider (status-bar display).
pub fn active_account_email(config: &AppConfig, provider: &str) -> Option<String> {
    let family = collect_accounts(config, provider).ok()?;
    let (key, credential) = family.first()?;
    if key != provider {
        return None;
    }
    credential_email(credential)
}

/// Make the account stored at `key` the active login for its provider.
/// Returns the new active account's label.
pub fn switch_account(config: &AppConfig, key: &str) -> Result<String> {
    let provider = account_provider(key).to_string();
    if !is_multi_account_provider(&provider) {
        bail!("multi-account login is only supported for anthropic and openai-codex");
    }
    let mut family = collect_accounts(config, &provider)?;
    let index = family
        .iter()
        .position(|(stored_key, _)| stored_key == key)
        .ok_or_else(|| anyhow!("no stored account {key} (see /accounts)"))?;
    let (_, chosen) = family.remove(index);
    let label = credential_label(&chosen);
    let mut ordered = vec![chosen];
    ordered.extend(family.into_iter().map(|(_, credential)| credential));
    write_family(config, &provider, &ordered)?;
    Ok(label)
}

/// Remove one stored account. When the active account is removed the next
/// stored one is promoted. Returns `(removed label, new active label)` or
/// `None` when nothing matched `key`.
pub fn logout_account(config: &AppConfig, key: &str) -> Result<Option<(String, Option<String>)>> {
    let provider = account_provider(key).to_string();
    let mut family = collect_accounts(config, &provider)?;
    let Some(index) = family.iter().position(|(stored_key, _)| stored_key == key) else {
        return Ok(None);
    };
    let (_, removed) = family.remove(index);
    let removed_label = credential_label(&removed);
    let ordered: Vec<StoredCredential> = family
        .into_iter()
        .map(|(_, credential)| credential)
        .collect();
    let next_active = ordered.first().map(credential_label);
    write_family(config, &provider, &ordered)?;
    Ok(Some((removed_label, next_active)))
}

/// Store a fresh OAuth login. Multi-account providers keep the previous
/// accounts (the new login becomes active; a re-login of the same account
/// replaces it in place). Other providers overwrite as before. Returns a
/// short summary for the chat transcript.
fn store_oauth_account(
    config: &AppConfig,
    provider: &str,
    credential: StoredCredential,
) -> Result<String> {
    if !is_multi_account_provider(provider) {
        write_credential(&primary_auth_path(config), provider, credential)?;
        return Ok(String::new());
    }
    let identity = credential_identity(&credential);
    let label = credential_label(&credential);
    let others: Vec<StoredCredential> = collect_accounts(config, provider)?
        .into_iter()
        .map(|(_, existing)| existing)
        .filter(
            |existing| match (&identity, credential_identity(existing)) {
                (Some(new_id), Some(existing_id)) => *new_id != existing_id,
                _ => true,
            },
        )
        .collect();
    let mut ordered = vec![credential];
    ordered.extend(others);
    let total = ordered.len();
    write_family(config, provider, &ordered)?;
    Ok(if total > 1 {
        format!("{label} is now active ({total} accounts stored — /accounts to switch)")
    } else {
        format!("{label} is now active")
    })
}

/// Fill in missing emails on logins stored before multi-login existed, so
/// `/accounts` and the status bar can name them. Codex emails come from the
/// token JWT (no network); Claude needs one profile call per account. Errors
/// are ignored — the display just keeps its generic label.
pub fn backfill_account_emails(config: &AppConfig) {
    for provider in MULTI_ACCOUNT_PROVIDERS {
        let Ok(family) = collect_accounts(config, provider) else {
            continue;
        };
        for (key, credential) in family {
            let StoredCredential::Oauth {
                access,
                refresh,
                expires,
                mut extra,
            } = credential
            else {
                continue;
            };
            if credential_email_from_extra(&extra).is_some() {
                continue;
            }
            let email = match *provider {
                "openai-codex" => openai_codex_email(&access),
                "anthropic" => Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .ok()
                    .and_then(|client| fetch_anthropic_profile_email(&client, &access)),
                _ => None,
            };
            let Some(email) = email else {
                continue;
            };
            extra.insert("email".to_string(), json!(email));
            let updated = StoredCredential::Oauth {
                access,
                refresh,
                expires,
                extra,
            };
            let _ = rewrite_existing_key(config, &key, updated);
        }
    }
}

/// Update a credential in place, in whichever store currently holds the key.
fn rewrite_existing_key(config: &AppConfig, key: &str, credential: StoredCredential) -> Result<()> {
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let mut data = read_auth_values(path)?;
        if data.contains_key(key) {
            data.insert(key.to_string(), serde_json::to_value(credential)?);
            return write_auth_values(path, &data);
        }
    }
    Ok(())
}

fn credential_label(credential: &StoredCredential) -> String {
    match credential {
        StoredCredential::ApiKey { .. } => "API key".to_string(),
        StoredCredential::Oauth { extra, .. } => credential_email_from_extra(extra)
            .or_else(|| {
                extra
                    .get("accountId")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(|id| format!("account {}", short_account_id(id)))
            })
            .unwrap_or_else(|| "OAuth account".to_string()),
    }
}

/// Short display form of an account id; a byte slice could split a multibyte
/// character and panic.
fn short_account_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn credential_email(credential: &StoredCredential) -> Option<String> {
    match credential {
        StoredCredential::ApiKey { .. } => None,
        StoredCredential::Oauth { extra, .. } => credential_email_from_extra(extra),
    }
}

fn credential_email_from_extra(extra: &BTreeMap<String, Value>) -> Option<String> {
    extra
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Identity used to detect a re-login of the same account (email, else the
/// Codex accountId).
fn credential_identity(credential: &StoredCredential) -> Option<String> {
    match credential {
        StoredCredential::ApiKey { .. } => None,
        StoredCredential::Oauth { extra, .. } => extra
            .get("email")
            .or_else(|| extra.get("accountId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase),
    }
}

#[derive(Debug, Clone)]
pub struct GithubCopilotModelConfig {
    pub base_url: String,
    pub available_model_ids: Option<Vec<String>>,
}

pub fn stored_api_key(config: &AppConfig, provider_id: &str) -> Result<Option<String>> {
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let data = read_auth(path)?;
        let Some(credential) = data.get(provider_id) else {
            continue;
        };
        match credential {
            StoredCredential::ApiKey { key, env } => {
                if let Some(value) = resolve_config_value(key.as_deref(), env) {
                    return Ok(Some(value));
                }
            }
            StoredCredential::Oauth {
                access,
                expires,
                refresh,
                extra,
            } => {
                if !access.trim().is_empty() && !is_expired(*expires) {
                    return Ok(Some(access.clone()));
                }
                let Some(refresh_token) = refresh.as_deref() else {
                    continue;
                };
                // Slot keys (`anthropic#2`) refresh with their base provider's
                // flow but persist back at their own key.
                if let Some(refreshed) =
                    refresh_oauth(account_provider(provider_id), refresh_token, extra)?
                {
                    write_refreshed(path, provider_id, refreshed.clone())?;
                    if let StoredCredential::Oauth { access, .. } = refreshed {
                        return Ok(Some(access));
                    }
                }
            }
        }
    }
    Ok(None)
}

pub fn stored_github_copilot_model_config(
    config: &AppConfig,
) -> Result<Option<GithubCopilotModelConfig>> {
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let data = read_auth(path)?;
        let Some(StoredCredential::Oauth { access, extra, .. }) = data.get("github-copilot") else {
            continue;
        };
        if access.trim().is_empty() {
            continue;
        }
        let enterprise = extra
            .get("enterpriseUrl")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let available_model_ids = extra.get("availableModelIds").and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
        });
        return Ok(Some(GithubCopilotModelConfig {
            base_url: copilot_base_url(access, enterprise),
            available_model_ids,
        }));
    }
    Ok(None)
}

pub fn stored_provider_env(
    config: &AppConfig,
    provider_id: &str,
) -> Result<BTreeMap<String, String>> {
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let data = read_auth(path)?;
        let Some(StoredCredential::ApiKey { env, .. }) = data.get(provider_id) else {
            continue;
        };
        return Ok(env.clone());
    }
    Ok(BTreeMap::new())
}

pub fn stored_openai_codex_account_id(config: &AppConfig) -> Result<Option<String>> {
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let data = read_auth(path)?;
        let Some(StoredCredential::Oauth { extra, .. }) = data.get("openai-codex") else {
            continue;
        };
        if let Some(account_id) = extra
            .get("accountId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Some(account_id.to_string()));
        }
    }
    Ok(None)
}

pub fn status_lines(config: &AppConfig) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    lines.push(format!("amazon-bedrock: {}", bedrock_auth_status()));
    // A single unreadable/corrupt store must not abort the whole listing.
    lines.push(format!(
        "google-vertex: {}",
        google_vertex_auth_status(config).unwrap_or_else(|err| format!("error: {err}"))
    ));
    for path in &config.auth_paths {
        if !path.exists() {
            lines.push(format!("{}: missing", path.display()));
            continue;
        }
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                lines.push(format!("{}: unreadable ({err})", path.display()));
                continue;
            }
        };
        let data: BTreeMap<String, StoredCredential> = match serde_json::from_str(&text) {
            Ok(data) => data,
            Err(err) => {
                lines.push(format!("{}: unparseable ({err})", path.display()));
                continue;
            }
        };
        if data.is_empty() {
            lines.push(format!("{}: empty", path.display()));
            continue;
        }
        lines.push(format!("{}:", path.display()));
        for (provider, credential) in data {
            let label = match credential {
                StoredCredential::ApiKey { key, env } => {
                    if key.as_deref().is_some_and(|value| !value.trim().is_empty()) {
                        if env.is_empty() {
                            "api_key configured".to_string()
                        } else {
                            format!("api_key configured, {} env values", env.len())
                        }
                    } else {
                        "api_key missing key".to_string()
                    }
                }
                StoredCredential::Oauth {
                    ref access,
                    ref refresh,
                    expires,
                    ref extra,
                } => {
                    let expiry = if is_expired(expires) {
                        "expired"
                    } else {
                        "valid-or-unbounded"
                    };
                    let refreshable = if refresh
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty())
                        && is_refresh_supported(account_provider(&provider))
                    {
                        ", refreshable"
                    } else {
                        ""
                    };
                    let account = credential_email_from_extra(extra)
                        .map(|email| format!(", {email}"))
                        .unwrap_or_default();
                    if access.trim().is_empty() {
                        format!("oauth missing access ({expiry}{refreshable}{account})")
                    } else {
                        format!("oauth {expiry}{refreshable}{account}")
                    }
                }
            };
            lines.push(format!("  {provider}: {label}"));
        }
    }
    Ok(lines)
}

fn bedrock_auth_status() -> String {
    if std::env::var("AWS_BEARER_TOKEN_BEDROCK")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return "AWS_BEARER_TOKEN_BEDROCK configured".to_string();
    }
    if std::env::var("AWS_ACCESS_KEY_ID")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
        && std::env::var("AWS_SECRET_ACCESS_KEY")
            .ok()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return "AWS access key env configured".to_string();
    }
    if std::env::var("AWS_PROFILE")
        .or_else(|_| std::env::var("AWS_DEFAULT_PROFILE"))
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return "AWS profile selected".to_string();
    }
    "not detected".to_string()
}

fn google_vertex_auth_status(config: &AppConfig) -> Result<String> {
    if stored_api_key(config, "google-vertex")?
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok("stored GOOGLE_CLOUD_API_KEY configured".to_string());
    }
    if std::env::var("GOOGLE_CLOUD_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok("GOOGLE_CLOUD_API_KEY configured".to_string());
    }
    let provider_env = stored_provider_env(config, "google-vertex")?;
    if provider_env
        .get("GOOGLE_OAUTH_ACCESS_TOKEN")
        .or_else(|| provider_env.get("GOOGLE_VERTEX_ACCESS_TOKEN"))
        .is_some_and(|value| !value.trim().is_empty())
        || std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN")
            .or_else(|_| std::env::var("GOOGLE_VERTEX_ACCESS_TOKEN"))
            .ok()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok("OAuth access token configured".to_string());
    }
    if let Some(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(std::path::PathBuf::from)
        && path.exists()
    {
        return Ok(format!("GOOGLE_APPLICATION_CREDENTIALS {}", path.display()));
    }
    if let Some(home) = dirs_next::home_dir() {
        let unix_adc = home
            .join(".config")
            .join("gcloud")
            .join("application_default_credentials.json");
        if unix_adc.exists() {
            return Ok(format!("ADC file {}", unix_adc.display()));
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            let windows_adc = std::path::PathBuf::from(appdata)
                .join("gcloud")
                .join("application_default_credentials.json");
            if windows_adc.exists() {
                return Ok(format!("ADC file {}", windows_adc.display()));
            }
        }
    }
    Ok("not detected".to_string())
}

/// Strip surrounding quotes and whitespace from a pasted API key. A key copied
/// with quotes or a trailing newline would otherwise be sent verbatim in the
/// `Authorization: Bearer` header and rejected as invalid.
pub fn normalize_api_key(key: &str) -> String {
    let trimmed = key.trim();
    let unquoted = if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    unquoted.trim().to_string()
}

pub fn store_api_key_with_env(
    config: &AppConfig,
    provider_id: &str,
    key: &str,
    env: BTreeMap<String, String>,
) -> Result<()> {
    let key = normalize_api_key(key);
    if provider_id.trim().is_empty() || key.is_empty() {
        bail!("usage: /login <provider> api-key <key>");
    }
    // Env values are secrets too (extra provider tokens/headers) — normalize
    // them the same way so a quoted/whitespaced paste doesn't break auth.
    let env = env
        .into_iter()
        .map(|(name, value)| (name, normalize_api_key(&value)))
        .collect();
    write_credential(
        &primary_auth_path(config),
        provider_id,
        StoredCredential::ApiKey {
            key: Some(key),
            env,
        },
    )
}

pub fn logout(config: &AppConfig, provider_id: &str) -> Result<bool> {
    let mut removed = false;
    for path in &config.auth_paths {
        if !path.exists() {
            continue;
        }
        let mut data = read_auth_values(path)?;
        if data.remove(provider_id).is_some() {
            write_auth_values(path, &data)?;
            removed = true;
        }
    }
    Ok(removed)
}

pub fn login_github_copilot_device(
    config: &AppConfig,
    enterprise_domain: Option<&str>,
) -> Result<()> {
    let client = Client::builder().build()?;
    let domain = enterprise_domain
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("github.com");
    let device = start_github_device_flow(&client, domain)?;
    println!(
        "\nOpen this URL in your browser:\n{}",
        device.verification_uri
    );
    println!("Enter code: {}\n", device.user_code);
    let github_access = poll_github_device_flow(&client, domain, &device)?;
    let mut extra = BTreeMap::new();
    if enterprise_domain.is_some() {
        extra.insert("enterpriseUrl".to_string(), json!(domain));
    }
    let credential = refresh_github_copilot(&client, &github_access, &extra)?;
    write_credential(&primary_auth_path(config), "github-copilot", credential)
}

pub fn login_openai_codex_device(config: &AppConfig) -> Result<String> {
    let client = Client::builder().build()?;
    let device = start_openai_codex_device_flow(&client)?;
    println!("\nOpen this URL in your browser:\n{OPENAI_CODEX_AUTH_BASE}/codex/device");
    println!("Enter code: {}\n", device.user_code);
    let token = poll_openai_codex_device_flow(&client, &device)?;
    let credential = exchange_openai_codex_authorization(
        &client,
        &token.authorization_code,
        &token.code_verifier,
        OPENAI_CODEX_DEVICE_REDIRECT_URI,
    )?;
    store_oauth_account(config, "openai-codex", credential)
}

pub fn login_openai_codex_browser(config: &AppConfig) -> Result<String> {
    let client = Client::builder().build()?;
    let verifier = openai_codex_pkce_verifier();
    let challenge = openai_codex_pkce_challenge(&verifier);
    let state = Uuid::new_v4().simple().to_string();
    let url = openai_codex_authorize_url(&challenge, &state);

    let host = oauth_callback_host();

    let opened = open_browser(&url).is_ok();
    print_auth_url(&url, opened);

    let code = match TcpListener::bind((host.as_str(), 1455)) {
        Ok(listener) => wait_for_openai_codex_callback(listener, &state)?,
        Err(error) => {
            eprintln!(
                "Could not bind OAuth callback on {host}:1455 ({error}). Paste the authorization code or full redirect URL."
            );
            prompt_for_openai_codex_code(Some(&state))?
        }
    };

    let credential = exchange_openai_codex_authorization(
        &client,
        &code,
        &verifier,
        OPENAI_CODEX_BROWSER_REDIRECT_URI,
    )?;
    store_oauth_account(config, "openai-codex", credential)
}

// Disabled: /login anthropic no longer starts this flow (API keys only), but
// the implementation is kept intact for re-enabling from commands::login.
#[allow(dead_code)]
pub fn login_anthropic_browser(config: &AppConfig) -> Result<String> {
    let client = Client::builder().build()?;
    let verifier = oauth_pkce_verifier();
    let challenge = oauth_pkce_challenge(&verifier);
    let url = anthropic_authorize_url(&challenge, &verifier);
    let host = oauth_callback_host();

    let opened = open_browser(&url).is_ok();
    print_auth_url(&url, opened);

    let code = match TcpListener::bind((host.as_str(), ANTHROPIC_CALLBACK_PORT)) {
        Ok(listener) => wait_for_anthropic_callback(listener, &verifier)?,
        Err(error) => {
            eprintln!(
                "Could not bind OAuth callback on {host}:{ANTHROPIC_CALLBACK_PORT} ({error}). Paste the authorization code or full redirect URL."
            );
            let (code, state) = prompt_for_oauth_code(Some(&verifier))?;
            if state.is_none() {
                eprintln!("No OAuth state was pasted; using the original PKCE verifier as state.");
            }
            code
        }
    };

    let credential = exchange_anthropic_authorization(&client, &code, &verifier, &verifier)?;
    store_oauth_account(config, "anthropic", credential)
}

fn resolve_config_value(
    value: Option<&str>,
    env_overlay: &BTreeMap<String, String>,
) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    // `!command` runs a shell command and uses its trimmed stdout (secrets).
    if let Some(command) = value.strip_prefix('!') {
        let command = command.trim();
        if command.is_empty() {
            return None;
        }
        let output = if cfg!(windows) {
            crate::spawn::no_window_command("cmd")
                .args(["/C", command])
                .output()
        } else {
            Command::new("sh").args(["-c", command]).output()
        };
        return output
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|text| !text.is_empty());
    }
    if let Some(name) = value.strip_prefix('$') {
        let name = name.trim_matches(|ch| ch == '{' || ch == '}');
        return env_overlay
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok());
    }
    Some(value.to_string())
}

fn read_auth(path: &Path) -> Result<BTreeMap<String, StoredCredential>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read auth store {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse auth store {}", path.display()))
}

fn write_refreshed(path: &Path, provider_id: &str, credential: StoredCredential) -> Result<()> {
    write_credential(path, provider_id, credential)
}

fn write_credential(path: &Path, provider_id: &str, credential: StoredCredential) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut data = read_auth_values(path)?;
    data.insert(provider_id.to_string(), serde_json::to_value(credential)?);
    write_auth_values(path, &data)
}

fn write_auth_values(path: &Path, data: &BTreeMap<String, Value>) -> Result<()> {
    let tmp = tmp_path(path);
    fs::write(&tmp, serde_json::to_string_pretty(&data)?)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    restrict_auth_permissions(&tmp)?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to replace auth store {}", path.display()))?;
    Ok(())
}

fn restrict_auth_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn read_auth_values(path: &Path) -> Result<BTreeMap<String, Value>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read auth store {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse auth store {}", path.display()))
}

fn primary_auth_path(config: &AppConfig) -> PathBuf {
    config
        .auth_paths
        .last()
        .cloned()
        .unwrap_or_else(|| config.user_app_dir.join("auth.json"))
}

fn tmp_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| format!("{value}."))
            .unwrap_or_default()
    ))
}

fn refresh_oauth(
    provider_id: &str,
    refresh_token: &str,
    extra: &BTreeMap<String, Value>,
) -> Result<Option<StoredCredential>> {
    let client = Client::builder().build()?;
    match provider_id {
        "anthropic" => refresh_anthropic(&client, refresh_token, extra).map(Some),
        "openai-codex" => refresh_openai_codex(&client, refresh_token).map(Some),
        "github-copilot" => refresh_github_copilot(&client, refresh_token, extra).map(Some),
        _ => Ok(None),
    }
}

fn is_refresh_supported(provider_id: &str) -> bool {
    matches!(provider_id, "anthropic" | "openai-codex" | "github-copilot")
}

#[derive(Debug, Deserialize)]
struct GithubDeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: Option<u64>,
    expires_in: u64,
}

#[derive(Debug)]
struct OpenAiCodexDeviceResponse {
    device_auth_id: String,
    user_code: String,
    interval: u64,
}

#[derive(Debug)]
struct OpenAiCodexDeviceToken {
    authorization_code: String,
    code_verifier: String,
}

fn start_github_device_flow(client: &Client, domain: &str) -> Result<GithubDeviceResponse> {
    let response: GithubDeviceResponse = client
        .post(format!("https://{domain}/login/device/code"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .form(&[
            ("client_id", "Iv1.b507a08c87ecfe98"),
            ("scope", "read:user"),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    validate_verification_uri(&response.verification_uri)?;
    Ok(response)
}

fn poll_github_device_flow(
    client: &Client,
    domain: &str,
    device: &GithubDeviceResponse,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(device.expires_in);
    let mut interval = Duration::from_secs(device.interval.unwrap_or(5).max(1));
    loop {
        if Instant::now() >= deadline {
            bail!("device flow timed out");
        }
        let response: Value = client
            .post(format!("https://{domain}/login/oauth/access_token"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", "GitHubCopilotChat/0.35.0")
            .form(&[
                ("client_id", "Iv1.b507a08c87ecfe98"),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()?
            .error_for_status()?
            .json()?;
        if let Some(token) = response["access_token"].as_str() {
            return Ok(token.to_string());
        }
        match response["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += Duration::from_secs(5),
            Some(error) => {
                let description = response["error_description"].as_str().unwrap_or("");
                bail!("device flow failed: {error} {description}");
            }
            None => bail!("invalid device token response: {response}"),
        }
        thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn start_openai_codex_device_flow(client: &Client) -> Result<OpenAiCodexDeviceResponse> {
    let response: Value = client
        .post("https://auth.openai.com/api/accounts/deviceauth/usercode")
        .json(&json!({"client_id": OPENAI_CODEX_CLIENT_ID}))
        .send()?
        .error_for_status()?
        .json()?;
    let interval = response["interval"]
        .as_u64()
        .or_else(|| {
            response["interval"]
                .as_str()
                .and_then(|value| value.parse().ok())
        })
        .ok_or_else(|| anyhow!("invalid OpenAI Codex device response: {response}"))?;
    let device_auth_id = response["device_auth_id"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid OpenAI Codex device response: {response}"))?
        .to_string();
    let user_code = response["user_code"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid OpenAI Codex device response: {response}"))?
        .to_string();
    Ok(OpenAiCodexDeviceResponse {
        device_auth_id,
        user_code,
        interval,
    })
}

fn poll_openai_codex_device_flow(
    client: &Client,
    device: &OpenAiCodexDeviceResponse,
) -> Result<OpenAiCodexDeviceToken> {
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    let mut interval = Duration::from_secs(device.interval.max(1));
    loop {
        if Instant::now() >= deadline {
            bail!("device flow timed out");
        }
        let response = client
            .post("https://auth.openai.com/api/accounts/deviceauth/token")
            .json(&json!({
                "device_auth_id": device.device_auth_id,
                "user_code": device.user_code
            }))
            .send()?;
        if response.status().is_success() {
            let value: Value = response.json()?;
            let authorization_code = value["authorization_code"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid OpenAI Codex token response: {value}"))?
                .to_string();
            let code_verifier = value["code_verifier"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid OpenAI Codex token response: {value}"))?
                .to_string();
            return Ok(OpenAiCodexDeviceToken {
                authorization_code,
                code_verifier,
            });
        }
        if response.status().as_u16() == 403 || response.status().as_u16() == 404 {
            thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
            continue;
        }
        let text = response.text().unwrap_or_default();
        if text.contains("deviceauth_authorization_pending") {
            thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
            continue;
        }
        if text.contains("slow_down") {
            interval += Duration::from_secs(5);
            thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
            continue;
        }
        bail!("OpenAI Codex device auth failed: {text}");
    }
}

fn exchange_openai_codex_authorization(
    client: &Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<StoredCredential> {
    #[derive(Deserialize)]
    struct Response {
        access_token: String,
        refresh_token: String,
        expires_in: i64,
        #[serde(default)]
        id_token: Option<String>,
    }
    let response: Response = client
        .post(OPENAI_CODEX_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", OPENAI_CODEX_CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let extra = openai_codex_extra(&response.access_token, response.id_token.as_deref())?;
    Ok(oauth_credential(
        response.access_token,
        response.refresh_token,
        response.expires_in,
        extra,
    ))
}

fn exchange_anthropic_authorization(
    client: &Client,
    code: &str,
    state: &str,
    verifier: &str,
) -> Result<StoredCredential> {
    let response: Value = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Accept", "application/json")
        .json(&json!({
            "grant_type": "authorization_code",
            "client_id": ANTHROPIC_CLIENT_ID,
            "code": code,
            "state": state,
            "redirect_uri": ANTHROPIC_REDIRECT_URI,
            "code_verifier": verifier
        }))
        .send()?
        .error_for_status()?
        .json()?;
    let access = response["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid Anthropic token response"))?
        .to_string();
    let refresh = response["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid Anthropic token response"))?
        .to_string();
    let expires_in = response["expires_in"].as_i64().unwrap_or(0);
    let mut extra = BTreeMap::new();
    let email = response["account"]["email_address"]
        .as_str()
        .or_else(|| response["account"]["email"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| fetch_anthropic_profile_email(client, &access));
    if let Some(email) = email {
        extra.insert("email".to_string(), json!(email));
    }
    Ok(oauth_credential(access, refresh, expires_in, extra))
}

/// Best-effort account email lookup for the `/accounts` display; the login
/// still succeeds when this endpoint is unavailable.
fn fetch_anthropic_profile_email(client: &Client, access: &str) -> Option<String> {
    let response: Value = client
        .get("https://api.anthropic.com/api/oauth/profile")
        .bearer_auth(access)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-cli/2.1.75")
        .header("x-app", "cli")
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    response["account"]["email_address"]
        .as_str()
        .or_else(|| response["account"]["email"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn refresh_anthropic(
    client: &Client,
    refresh_token: &str,
    extra: &BTreeMap<String, Value>,
) -> Result<StoredCredential> {
    #[derive(Deserialize)]
    struct Response {
        access_token: String,
        refresh_token: String,
        expires_in: i64,
    }
    let response: Response = client
        .post(ANTHROPIC_TOKEN_URL)
        .json(&json!({
            "grant_type": "refresh_token",
            "client_id": ANTHROPIC_CLIENT_ID,
            "refresh_token": refresh_token
        }))
        .send()?
        .error_for_status()?
        .json()?;
    Ok(oauth_credential(
        response.access_token,
        response.refresh_token,
        response.expires_in,
        // Keep the account identity (email) across refreshes.
        extra.clone(),
    ))
}

fn refresh_openai_codex(client: &Client, refresh_token: &str) -> Result<StoredCredential> {
    #[derive(Deserialize)]
    struct Response {
        access_token: String,
        refresh_token: String,
        expires_in: i64,
        #[serde(default)]
        id_token: Option<String>,
    }
    let response: Response = client
        .post(OPENAI_CODEX_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OPENAI_CODEX_CLIENT_ID),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let extra = openai_codex_extra(&response.access_token, response.id_token.as_deref())?;
    Ok(oauth_credential(
        response.access_token,
        response.refresh_token,
        response.expires_in,
        extra,
    ))
}

fn refresh_github_copilot(
    client: &Client,
    refresh_token: &str,
    extra: &BTreeMap<String, Value>,
) -> Result<StoredCredential> {
    #[derive(Deserialize)]
    struct Response {
        token: String,
        expires_at: i64,
    }
    let enterprise = extra
        .get("enterpriseUrl")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let domain = enterprise.unwrap_or("github.com");
    let url = format!("https://api.{domain}/copilot_internal/v2/token");
    let response: Response = client
        .get(url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {refresh_token}"))
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .header("Editor-Version", "vscode/1.107.0")
        .header("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .header("Copilot-Integration-Id", "vscode-chat")
        .send()?
        .error_for_status()?
        .json()?;
    let mut next_extra = extra.clone();
    if let Some(enterprise) = enterprise {
        next_extra.insert("enterpriseUrl".to_string(), json!(enterprise));
    }
    let available_model_ids = fetch_copilot_model_ids(client, &response.token, enterprise)?;
    next_extra.insert("availableModelIds".to_string(), json!(available_model_ids));
    Ok(StoredCredential::Oauth {
        access: response.token,
        refresh: Some(refresh_token.to_string()),
        expires: Some(response.expires_at * 1000 - 5 * 60 * 1000),
        extra: next_extra,
    })
}

fn fetch_copilot_model_ids(
    client: &Client,
    token: &str,
    enterprise: Option<&str>,
) -> Result<Vec<String>> {
    let base_url = copilot_base_url(token, enterprise);
    let response: Value = client
        .get(format!("{base_url}/models"))
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .header("Editor-Version", "vscode/1.107.0")
        .header("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .header("Copilot-Integration-Id", "vscode-chat")
        .header("X-GitHub-Api-Version", "2026-06-01")
        .send()?
        .error_for_status()?
        .json()?;
    let mut ids = Vec::new();
    for item in response["data"].as_array().into_iter().flatten() {
        let id = item["id"].as_str();
        let model_picker_enabled = item["model_picker_enabled"].as_bool() == Some(true);
        let policy_enabled = item["policy"]["state"].as_str() != Some("disabled");
        let supports_tool_calls =
            item["capabilities"]["supports"]["tool_calls"].as_bool() != Some(false);
        if let Some(id) = id
            && model_picker_enabled
            && policy_enabled
            && supports_tool_calls
        {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

fn copilot_base_url(token: &str, enterprise: Option<&str>) -> String {
    if let Some(proxy) = token
        .split(';')
        .find_map(|part| part.trim().strip_prefix("proxy-ep="))
    {
        return format!("https://{}", proxy.replacen("proxy.", "api.", 1));
    }
    enterprise
        .map(|domain| format!("https://copilot-api.{domain}"))
        .unwrap_or_else(|| "https://api.individual.githubcopilot.com".to_string())
}

fn oauth_callback_host() -> String {
    std::env::var("PI_OAUTH_CALLBACK_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn oauth_pkce_verifier() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn oauth_pkce_challenge(verifier: &str) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn openai_codex_pkce_verifier() -> String {
    oauth_pkce_verifier()
}

fn openai_codex_pkce_challenge(verifier: &str) -> String {
    oauth_pkce_challenge(verifier)
}

fn openai_codex_authorize_url(challenge: &str, state: &str) -> String {
    format!(
        "{OPENAI_CODEX_AUTHORIZE_URL}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&id_token_add_organizations=true&codex_cli_simplified_flow=true&originator=pi",
        urlencoding::encode(OPENAI_CODEX_CLIENT_ID),
        urlencoding::encode(OPENAI_CODEX_BROWSER_REDIRECT_URI),
        urlencoding::encode(OPENAI_CODEX_SCOPE),
        urlencoding::encode(challenge),
        urlencoding::encode(state),
    )
}

fn anthropic_authorize_url(challenge: &str, state: &str) -> String {
    format!(
        "{ANTHROPIC_AUTHORIZE_URL}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        urlencoding::encode(ANTHROPIC_CLIENT_ID),
        urlencoding::encode(ANTHROPIC_REDIRECT_URI),
        urlencoding::encode(ANTHROPIC_SCOPE),
        urlencoding::encode(challenge),
        urlencoding::encode(state),
    )
}

fn wait_for_openai_codex_callback(listener: TcpListener, state: &str) -> Result<String> {
    listener
        .set_nonblocking(true)
        .context("failed to configure OAuth callback listener")?;
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                // The accepted socket inherits non-blocking mode on Windows;
                // switch to a blocking read with a timeout, and ignore stray
                // connections so the real callback isn't aborted by them.
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                if let Ok(Some(code)) = handle_openai_codex_callback(&mut stream, state) {
                    return Ok(code);
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error).context("OAuth callback listener failed"),
        }
    }
    bail!("OpenAI Codex browser login timed out");
}

fn wait_for_anthropic_callback(listener: TcpListener, state: &str) -> Result<String> {
    listener
        .set_nonblocking(true)
        .context("failed to configure OAuth callback listener")?;
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                // The accepted socket inherits non-blocking mode on Windows;
                // switch to a blocking read with a timeout, and ignore stray
                // connections so the real callback isn't aborted by them.
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                if let Ok(Some(code)) = handle_anthropic_callback(&mut stream, state) {
                    return Ok(code);
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error).context("OAuth callback listener failed"),
        }
    }
    bail!("Anthropic browser login timed out");
}

fn handle_openai_codex_callback(stream: &mut TcpStream, state: &str) -> Result<Option<String>> {
    let mut buffer = [0_u8; 8192];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        write_oauth_response(
            stream,
            405,
            oauth_error_html("Unsupported callback method."),
        )?;
        return Ok(None);
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/auth/callback" {
        write_oauth_response(stream, 404, oauth_error_html("Callback route not found."))?;
        return Ok(None);
    }
    if query_param(query, "state").as_deref() != Some(state) {
        write_oauth_response(stream, 400, oauth_error_html("State mismatch."))?;
        return Ok(None);
    }
    let Some(code) = query_param(query, "code").filter(|value| !value.trim().is_empty()) else {
        write_oauth_response(stream, 400, oauth_error_html("Missing authorization code."))?;
        return Ok(None);
    };
    write_oauth_response(
        stream,
        200,
        oauth_success_html("OpenAI sign-in done — this tab can be closed now."),
    )?;
    Ok(Some(code))
}

fn handle_anthropic_callback(stream: &mut TcpStream, state: &str) -> Result<Option<String>> {
    let mut buffer = [0_u8; 8192];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        write_oauth_response(
            stream,
            405,
            oauth_error_html("Unsupported callback method."),
        )?;
        return Ok(None);
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != ANTHROPIC_CALLBACK_PATH {
        write_oauth_response(stream, 404, oauth_error_html("Callback route not found."))?;
        return Ok(None);
    }
    if let Some(error) = query_param(query, "error") {
        write_oauth_response(
            stream,
            400,
            oauth_error_html(&format!(
                "Anthropic authentication did not complete: {error}"
            )),
        )?;
        return Ok(None);
    }
    if query_param(query, "state").as_deref() != Some(state) {
        write_oauth_response(stream, 400, oauth_error_html("State mismatch."))?;
        return Ok(None);
    }
    let Some(code) = query_param(query, "code").filter(|value| !value.trim().is_empty()) else {
        write_oauth_response(stream, 400, oauth_error_html("Missing authorization code."))?;
        return Ok(None);
    };
    write_oauth_response(
        stream,
        200,
        oauth_success_html("Anthropic sign-in done — this tab can be closed now."),
    )?;
    Ok(Some(code))
}

/// Read a line from `reader`, terminating on the first CR or LF. `read_line`
/// only stops at LF, but xterm.js sends a bare CR for Enter and Windows ConPTY
/// may deliver CR-only input on this manual-paste fallback, which would hang
/// read_line forever (terminal-win#6). Reading byte-wise until either
/// terminator avoids that.
fn read_line_cr_or_lf(reader: &mut impl Read) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\r' || byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            Err(ref error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn prompt_for_openai_codex_code(expected_state: Option<&str>) -> Result<String> {
    print!("Authorization code or redirect URL: ");
    std::io::stdout().flush()?;
    let input = read_line_cr_or_lf(&mut std::io::stdin().lock())?;
    let (code, state) = parse_oauth_authorization_input(input.trim())?;
    if let (Some(expected), Some(actual)) = (expected_state, state.as_deref())
        && expected != actual
    {
        bail!("OpenAI Codex OAuth state mismatch");
    }
    code.filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("missing authorization code"))
}

fn prompt_for_oauth_code(expected_state: Option<&str>) -> Result<(String, Option<String>)> {
    print!("Authorization code or redirect URL: ");
    std::io::stdout().flush()?;
    let input = read_line_cr_or_lf(&mut std::io::stdin().lock())?;
    let (code, state) = parse_oauth_authorization_input(input.trim())?;
    if let (Some(expected), Some(actual)) = (expected_state, state.as_deref())
        && expected != actual
    {
        bail!("OAuth state mismatch");
    }
    let code = code
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("missing authorization code"))?;
    Ok((code, state))
}

fn parse_oauth_authorization_input(input: &str) -> Result<(Option<String>, Option<String>)> {
    let value = input.trim();
    if value.is_empty() {
        return Ok((None, None));
    }
    if let Some(query) = value
        .strip_prefix(OPENAI_CODEX_BROWSER_REDIRECT_URI)
        .and_then(|tail| tail.strip_prefix('?'))
        .or_else(|| value.split_once('?').map(|(_, query)| query))
    {
        return Ok((query_param(query, "code"), query_param(query, "state")));
    }
    if let Some((code, state)) = value.split_once('#') {
        return Ok((Some(code.to_string()), Some(state.to_string())));
    }
    if value.contains("code=") {
        return Ok((query_param(value, "code"), query_param(value, "state")));
    }
    Ok((Some(value.to_string()), None))
}

fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let decoded_key = urlencoding::decode(raw_key).ok()?;
        if decoded_key == key {
            return urlencoding::decode(raw_value)
                .ok()
                .map(|value| value.into_owned());
        }
    }
    None
}

fn write_oauth_response(stream: &mut TcpStream, status: u16, html: String) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    )?;
    stream.flush()?;
    Ok(())
}

fn oauth_success_html(message: &str) -> String {
    oauth_page_html("OAuth Login Complete", message)
}

fn oauth_error_html(message: &str) -> String {
    oauth_page_html("OAuth Login Failed", message)
}

fn oauth_page_html(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{font-family:system-ui,sans-serif;margin:48px;color:#1f2933}}main{{max-width:720px}}h1{{font-size:24px}}</style></head><body><main><h1>{}</h1><p>{}</p></main></body></html>",
        html_escape(title),
        html_escape(title),
        html_escape(message)
    )
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

/// Show the OAuth URL prominently so the user can always copy it, whether or
/// not the browser opened automatically.
fn print_auth_url(url: &str, opened: bool) {
    let bar = "─".repeat(70);
    println!("\n{bar}");
    if opened {
        println!(
            "Opened your browser. If nothing appeared, or the wrong window opened, copy the URL below and paste it into your browser:"
        );
    } else {
        println!(
            "Could not open a browser automatically. Copy the URL below and paste it into your browser:"
        );
    }
    println!("{bar}\n");
    println!("{url}\n");
    println!("{bar}\n");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Open a URL in the default browser (used by the TUI link list).
pub fn open_url(url: &str) -> Result<()> {
    open_browser(url)
}

fn open_browser(url: &str) -> Result<()> {
    let mut command = if cfg!(target_os = "windows") {
        // rundll32 FileProtocolHandler reliably opens an http(s) URL (including
        // query strings) in the default browser; explorer.exe sometimes routes
        // such URLs to the wrong handler (a document/folder window).
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .context("failed to open browser")
}

fn validate_verification_uri(uri: &str) -> Result<()> {
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Ok(());
    }
    bail!("untrusted verification_uri in device code response");
}

fn jwt_claims(token: &str) -> Option<Value> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

pub fn openai_codex_account_id(access_token: &str) -> Option<String> {
    jwt_claims(access_token)?["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// ChatGPT login email, from the access token's profile claim (or the plain
/// `email` claim on id_tokens).
fn openai_codex_email(token: &str) -> Option<String> {
    let claims = jwt_claims(token)?;
    claims["https://api.openai.com/profile"]["email"]
        .as_str()
        .or_else(|| claims["email"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn openai_codex_extra(
    access_token: &str,
    id_token: Option<&str>,
) -> Result<BTreeMap<String, Value>> {
    let account_id = openai_codex_account_id(access_token)
        .ok_or_else(|| anyhow!("failed to extract OpenAI Codex accountId from access token"))?;
    let mut extra = BTreeMap::new();
    extra.insert("accountId".to_string(), json!(account_id));
    if let Some(email) =
        openai_codex_email(access_token).or_else(|| id_token.and_then(openai_codex_email))
    {
        extra.insert("email".to_string(), json!(email));
    }
    Ok(extra)
}

fn oauth_credential(
    access: String,
    refresh: String,
    expires_in_seconds: i64,
    extra: BTreeMap<String, Value>,
) -> StoredCredential {
    StoredCredential::Oauth {
        access,
        refresh: Some(refresh),
        expires: Some(
            chrono::Utc::now().timestamp_millis() + expires_in_seconds * 1000 - 5 * 60 * 1000,
        ),
        extra,
    }
}

fn is_expired(expires: Option<i64>) -> bool {
    let Some(expires) = expires else {
        return false;
    };
    let now = chrono::Utc::now().timestamp_millis();
    now >= expires
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn read_line_cr_or_lf_stops_at_either_terminator() {
        use std::io::Cursor;
        assert_eq!(
            read_line_cr_or_lf(&mut Cursor::new(b"abc\n".to_vec())).unwrap(),
            "abc"
        );
        // xterm.js / Windows ConPTY Enter arrives as a bare CR (terminal-win#6).
        assert_eq!(
            read_line_cr_or_lf(&mut Cursor::new(b"abc\r".to_vec())).unwrap(),
            "abc"
        );
        // CRLF stops at the CR; the caller trims any leftover.
        assert_eq!(
            read_line_cr_or_lf(&mut Cursor::new(b"abc\r\n".to_vec())).unwrap(),
            "abc"
        );
        // EOF without a terminator returns what was read.
        assert_eq!(
            read_line_cr_or_lf(&mut Cursor::new(b"abc".to_vec())).unwrap(),
            "abc"
        );
        // An immediate terminator yields an empty line.
        assert_eq!(
            read_line_cr_or_lf(&mut Cursor::new(b"\r".to_vec())).unwrap(),
            ""
        );
    }

    #[test]
    fn has_any_login_detects_stored_and_env_credentials() {
        use std::sync::Mutex;
        static ENV_GUARD: Mutex<()> = Mutex::new(());
        let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        const KEYS: &[&str] = &[
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "OPENROUTER_API_KEY",
            "GROQ_API_KEY",
            "XAI_API_KEY",
            "DEEPSEEK_API_KEY",
            "MISTRAL_API_KEY",
            "DASHSCOPE_API_KEY",
            "ZAI_API_KEY",
            "MOONSHOT_API_KEY",
        ];
        let saved: Vec<(&str, Option<String>)> =
            KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        for k in KEYS {
            unsafe { std::env::remove_var(k) };
        }

        // Fresh: no stored login, no env key → onboarding should trigger.
        let (config, _dir) = test_config("has-any-login-fresh");
        assert!(!has_any_login(&config));
        // A stored API key counts as logged in.
        store_api_key_with_env(&config, "xai", "xai-abc", BTreeMap::new()).unwrap();
        assert!(has_any_login(&config));
        // A provider env key alone counts too, on an otherwise-fresh config.
        let (fresh, _d2) = test_config("has-any-login-env");
        assert!(!has_any_login(&fresh));
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test") };
        assert!(has_any_login(&fresh));

        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
        for (k, v) in saved {
            match v {
                Some(v) => unsafe { std::env::set_var(k, v) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
    }

    #[test]
    fn normalize_api_key_strips_quotes_and_whitespace() {
        assert_eq!(normalize_api_key("  xai-abc123  "), "xai-abc123");
        assert_eq!(normalize_api_key("\"xai-abc123\""), "xai-abc123");
        assert_eq!(normalize_api_key("'xai-abc123'"), "xai-abc123");
        assert_eq!(normalize_api_key("xai-abc123\n"), "xai-abc123");
        assert_eq!(normalize_api_key("sk-plain"), "sk-plain");
    }

    #[test]
    fn stored_key_and_env_values_are_normalized() {
        let (config, dir) = test_config("normalize-store");
        let mut env = BTreeMap::new();
        env.insert("EXTRA_TOKEN".to_string(), "'  tok-123  '".to_string());
        store_api_key_with_env(&config, "xai", "\" xai-abc \"", env).unwrap();
        assert_eq!(
            stored_api_key(&config, "xai").unwrap().as_deref(),
            Some("xai-abc")
        );
        assert_eq!(
            stored_provider_env(&config, "xai")
                .unwrap()
                .get("EXTRA_TOKEN")
                .map(String::as_str),
            Some("tok-123")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn test_config(name: &str) -> (AppConfig, PathBuf) {
        let dir = std::env::temp_dir().join(format!("bbarit-auth-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut config = AppConfig::for_test(dir.clone());
        config.auth_paths = vec![dir.join("auth.json")];
        (config, dir)
    }

    fn oauth(email: &str) -> StoredCredential {
        let mut extra = BTreeMap::new();
        extra.insert("email".to_string(), json!(email));
        StoredCredential::Oauth {
            access: format!("token-{email}"),
            refresh: Some("refresh".to_string()),
            expires: None,
            extra,
        }
    }

    fn stored_access(config: &AppConfig, key: &str) -> Option<String> {
        let data = read_auth(&config.auth_paths[0]).unwrap();
        match data.get(key)? {
            StoredCredential::Oauth { access, .. } => Some(access.clone()),
            StoredCredential::ApiKey { key, .. } => key.clone(),
        }
    }

    #[test]
    fn second_login_keeps_first_account_and_activates_new() {
        let (config, dir) = test_config("second-login");
        store_oauth_account(&config, "anthropic", oauth("a@x.com")).unwrap();
        let summary = store_oauth_account(&config, "anthropic", oauth("b@x.com")).unwrap();
        assert!(summary.contains("b@x.com"), "{summary}");

        let accounts = list_accounts(&config, "anthropic").unwrap();
        assert_eq!(accounts.len(), 2);
        assert!(accounts[0].active);
        assert_eq!(accounts[0].label, "b@x.com");
        assert_eq!(accounts[1].label, "a@x.com");
        assert_eq!(accounts[1].key, "anthropic#2");
        // The active credential stays at the plain key every consumer reads.
        assert_eq!(
            stored_access(&config, "anthropic").as_deref(),
            Some("token-b@x.com")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn relogin_of_same_account_does_not_duplicate() {
        let (config, dir) = test_config("relogin-dedupe");
        store_oauth_account(&config, "anthropic", oauth("a@x.com")).unwrap();
        store_oauth_account(&config, "anthropic", oauth("A@X.com")).unwrap();
        assert_eq!(list_accounts(&config, "anthropic").unwrap().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn switch_account_swaps_active_slot() {
        let (config, dir) = test_config("switch");
        store_oauth_account(&config, "openai-codex", oauth("a@x.com")).unwrap();
        store_oauth_account(&config, "openai-codex", oauth("b@x.com")).unwrap();

        let label = switch_account(&config, "openai-codex#2").unwrap();
        assert_eq!(label, "a@x.com");
        let accounts = list_accounts(&config, "openai-codex").unwrap();
        assert_eq!(accounts[0].label, "a@x.com");
        assert!(accounts[0].active);
        assert_eq!(accounts[1].label, "b@x.com");
        assert_eq!(
            stored_access(&config, "openai-codex").as_deref(),
            Some("token-a@x.com")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn switch_rejects_single_account_providers() {
        let (config, dir) = test_config("switch-reject");
        assert!(switch_account(&config, "openrouter#2").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn logout_active_promotes_next_account() {
        let (config, dir) = test_config("logout-promote");
        store_oauth_account(&config, "anthropic", oauth("a@x.com")).unwrap();
        store_oauth_account(&config, "anthropic", oauth("b@x.com")).unwrap();

        let (removed, next) = logout_account(&config, "anthropic").unwrap().unwrap();
        assert_eq!(removed, "b@x.com");
        assert_eq!(next.as_deref(), Some("a@x.com"));
        let accounts = list_accounts(&config, "anthropic").unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(accounts[0].active);
        assert_eq!(
            stored_access(&config, "anthropic").as_deref(),
            Some("token-a@x.com")
        );

        let (removed, next) = logout_account(&config, "anthropic").unwrap().unwrap();
        assert_eq!(removed, "a@x.com");
        assert!(next.is_none());
        assert!(list_accounts(&config, "anthropic").unwrap().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn family_keys_do_not_leak_across_providers() {
        assert!(family_key_matches("anthropic", "anthropic#2"));
        assert!(!family_key_matches("anthropic", "anthropic#"));
        assert!(!family_key_matches("anthropic", "anthropic#x"));
        assert!(!family_key_matches("anthropic", "anthropic-extra"));
        assert!(!family_key_matches("openai", "openai-codex"));
        assert_eq!(account_provider("openai-codex#3"), "openai-codex");
        assert_eq!(account_provider("anthropic"), "anthropic");
    }

    #[test]
    fn multi_account_keys_are_invisible_to_stored_api_key() {
        let (config, dir) = test_config("plain-key-only");
        store_oauth_account(&config, "anthropic", oauth("a@x.com")).unwrap();
        store_oauth_account(&config, "anthropic", oauth("b@x.com")).unwrap();
        // The LLM layer looks up plain provider ids, so it always gets the
        // active account's token; slot keys are reachable only by explicit key.
        assert_eq!(
            stored_api_key(&config, "anthropic").unwrap().as_deref(),
            Some("token-b@x.com")
        );
        assert_eq!(
            stored_api_key(&config, "anthropic#2").unwrap().as_deref(),
            Some("token-a@x.com")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn short_account_id_is_char_safe() {
        // Byte 8 falls inside a multibyte char; a byte slice would panic.
        assert_eq!(
            short_account_id("계정아이디등록번호구분"),
            "계정아이디등록번"
        );
        assert_eq!(short_account_id("acct-1234567"), "acct-123");
        assert_eq!(short_account_id("abc"), "abc");
    }

    #[test]
    fn status_lines_survive_corrupt_store() {
        let (mut config, dir) = test_config("corrupt-store");
        let bad = dir.join("bad.json");
        let good = dir.join("good.json");
        config.auth_paths = vec![bad.clone(), good.clone()];
        fs::write(&bad, "{not json").unwrap();
        write_credential(
            &good,
            "openrouter",
            StoredCredential::ApiKey {
                key: Some("key".to_string()),
                env: BTreeMap::new(),
            },
        )
        .unwrap();

        let lines = status_lines(&config).unwrap();
        assert!(
            lines.iter().any(|line| line.contains("unparseable")),
            "{lines:?}"
        );
        // The remaining stores are still listed.
        assert!(
            lines.iter().any(|line| line.contains("openrouter")),
            "{lines:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn codex_email_is_read_from_jwt_profile_claim() {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let payload = URL_SAFE_NO_PAD.encode(
            json!({
                "https://api.openai.com/profile": {"email": "codex@x.com"},
                "https://api.openai.com/auth": {"chatgpt_account_id": "acct-123"}
            })
            .to_string(),
        );
        let token = format!("h.{payload}.s");
        assert_eq!(openai_codex_email(&token).as_deref(), Some("codex@x.com"));
        assert_eq!(openai_codex_account_id(&token).as_deref(), Some("acct-123"));
        let extra = openai_codex_extra(&token, None).unwrap();
        assert_eq!(
            extra.get("email").and_then(Value::as_str),
            Some("codex@x.com")
        );
    }
}
