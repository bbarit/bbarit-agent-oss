//! Subscription usage / remaining-quota lookup for OAuth logins.
//!
//! Claude (Anthropic) and Codex (ChatGPT) subscriptions meter usage in rolling
//! windows (5-hour and weekly) as a used percentage, not a token count — so
//! "remaining" is shown as the percent of the window still left plus the reset
//! time. Both endpoints are the same ones the official CLIs poll; they only
//! work with OAuth (browser-login) credentials, not raw API keys.

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::config::AppConfig;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Clone)]
pub struct UsageWindow {
    /// "5h", "week", "week Opus", …
    pub label: String,
    /// 0–100.
    pub used_percent: f64,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ProviderUsage {
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
}

/// Usage for the ACTIVE account of a provider. Only anthropic / openai-codex
/// meter subscription usage; only OAuth logins can query it.
pub fn fetch_usage(config: &AppConfig, provider_id: &str) -> Result<ProviderUsage> {
    fetch_usage_for_key(config, provider_id)
}

/// Usage for ANY stored account — the active one (`anthropic`) or a slot
/// (`anthropic#2`). Each account queries with its own (refreshed) token.
pub fn fetch_usage_for_key(config: &AppConfig, key: &str) -> Result<ProviderUsage> {
    match crate::auth::account_provider(key) {
        "anthropic" => {
            let token = crate::auth::stored_api_key(config, key)?
                .ok_or_else(|| anyhow!("not logged in"))?;
            if !token.contains("sk-ant-oat") {
                bail!("usage is only available for browser (OAuth) logins, not API keys");
            }
            fetch_claude_usage(&token)
        }
        "openai-codex" => {
            let token = crate::auth::stored_api_key(config, key)?
                .ok_or_else(|| anyhow!("not logged in"))?;
            let account_id = crate::auth::openai_codex_account_id(&token)
                .or(crate::auth::stored_openai_codex_account_id(config)?);
            fetch_codex_usage(&token, account_id.as_deref())
        }
        _ => bail!("usage display is only supported for anthropic and openai-codex"),
    }
}

/// The 5-hour session window (the one that throttles first), if reported.
pub fn session_window(usage: &ProviderUsage) -> Option<&UsageWindow> {
    usage
        .windows
        .iter()
        .find(|window| !window.label.starts_with("week"))
}

/// The weekly window, if reported.
pub fn week_window(usage: &ProviderUsage) -> Option<&UsageWindow> {
    usage
        .windows
        .iter()
        .find(|window| window.label.starts_with("week"))
}

/// Time until a reset, compact: "2h45m", "4d1h", "55m".
pub fn remaining_hint(resets_at_epoch: i64) -> String {
    let seconds = resets_at_epoch - chrono::Utc::now().timestamp();
    if seconds <= 0 {
        return "now".to_string();
    }
    let minutes = seconds / 60;
    let (days, hours, mins) = (minutes / 1440, (minutes % 1440) / 60, minutes % 60);
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{mins}m")
    } else {
        format!("{mins}m")
    }
}

fn usage_client() -> Result<Client> {
    Ok(Client::builder().timeout(Duration::from_secs(6)).build()?)
}

fn fetch_claude_usage(access_token: &str) -> Result<ProviderUsage> {
    let response: Value = usage_client()?
        .get(CLAUDE_USAGE_URL)
        .bearer_auth(access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-cli/2.1.75")
        .header("x-app", "cli")
        .send()?
        .error_for_status()?
        .json()?;
    Ok(parse_claude_usage(&response))
}

fn parse_claude_usage(value: &Value) -> ProviderUsage {
    let mut windows = Vec::new();
    for (field, label) in [
        ("five_hour", "5h"),
        ("seven_day", "week"),
        ("seven_day_opus", "week Opus"),
        ("seven_day_sonnet", "week Sonnet"),
    ] {
        let window = &value[field];
        let Some(used) = window["utilization"].as_f64() else {
            continue;
        };
        let resets_at = window["resets_at"]
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map(|at| at.timestamp())
            .or_else(|| window["resets_at"].as_i64());
        windows.push(UsageWindow {
            label: label.to_string(),
            used_percent: used,
            resets_at,
        });
    }
    ProviderUsage {
        plan: None,
        windows,
    }
}

fn fetch_codex_usage(access_token: &str, account_id: Option<&str>) -> Result<ProviderUsage> {
    let mut request = usage_client()?
        .get(CODEX_USAGE_URL)
        .bearer_auth(access_token)
        .header("originator", "pi")
        .header("User-Agent", "bbarit-oss (rust)")
        .header("Accept", "application/json");
    if let Some(account_id) = account_id {
        request = request.header("chatgpt-account-id", account_id);
    }
    let response: Value = request.send()?.error_for_status()?.json()?;
    Ok(parse_codex_usage(&response))
}

/// The wham/usage payload has shipped in both snake_case and camelCase.
fn parse_codex_usage(value: &Value) -> ProviderUsage {
    let limits = ["rate_limits", "rateLimits"]
        .iter()
        .map(|key| &value[*key])
        .find(|node| !node.is_null())
        .unwrap_or(&Value::Null);
    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let window = &limits[key];
        let Some(used) = window["used_percent"]
            .as_f64()
            .or_else(|| window["usedPercent"].as_f64())
        else {
            continue;
        };
        let minutes = window["window_minutes"]
            .as_i64()
            .or_else(|| window["windowDurationMins"].as_i64());
        let resets_at = window["resets_at"]
            .as_i64()
            .or_else(|| window["resetsAt"].as_i64());
        windows.push(UsageWindow {
            label: codex_window_label(minutes),
            used_percent: used,
            resets_at,
        });
    }
    let plan = value["plan_type"]
        .as_str()
        .or_else(|| value["planType"].as_str())
        .map(ToOwned::to_owned);
    ProviderUsage { plan, windows }
}

fn codex_window_label(minutes: Option<i64>) -> String {
    match minutes {
        Some(m) if m >= 10_080 => "week".to_string(),
        Some(m) if m >= 60 => format!("{}h", m / 60),
        Some(m) => format!("{m}m"),
        None => "window".to_string(),
    }
}

/// "5h 32% used · 68% left · resets 14:05 | week 12% used …" — the compact
/// one-line form used by /accounts and the accounts picker.
pub fn format_usage(usage: &ProviderUsage) -> String {
    if usage.windows.is_empty() {
        return "usage data unavailable".to_string();
    }
    let mut parts: Vec<String> = usage
        .windows
        .iter()
        .map(|window| {
            let left = (100.0 - window.used_percent).max(0.0);
            let reset = window
                .resets_at
                .map(format_reset)
                .map(|at| format!(" · resets {at}"))
                .unwrap_or_default();
            format!(
                "{} {:.0}% used · {:.0}% left{}",
                window.label, window.used_percent, left, reset
            )
        })
        .collect();
    if let Some(plan) = &usage.plan {
        parts.push(format!("plan {plan}"));
    }
    parts.join("  |  ")
}

/// Text-mode gauges for plain CLI output:
/// `5h ██████░░░░ 46% resets 2h45m · week █████████░ 94% resets 3h55m`
pub fn format_usage_bars(usage: &ProviderUsage) -> String {
    if usage.windows.is_empty() {
        return "usage data unavailable".to_string();
    }
    const WIDTH: usize = 10;
    let mut parts: Vec<String> = usage
        .windows
        .iter()
        .map(|window| {
            let pct = window.used_percent.clamp(0.0, 100.0);
            let filled = ((pct / 100.0) * WIDTH as f64).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(WIDTH - filled));
            let reset = window
                .resets_at
                .map(remaining_hint)
                .map(|hint| format!(" resets {hint}"))
                .unwrap_or_default();
            format!("{} {bar} {pct:>3.0}%{reset}", window.label)
        })
        .collect();
    if let Some(plan) = &usage.plan {
        parts.push(format!("plan {plan}"));
    }
    parts.join(" · ")
}

fn format_reset(epoch_seconds: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(epoch_seconds, 0) {
        chrono::LocalResult::Single(at) => {
            let now = chrono::Local::now();
            if at.date_naive() == now.date_naive() {
                at.format("%H:%M").to_string()
            } else {
                at.format("%m-%d %H:%M").to_string()
            }
        }
        _ => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_claude_usage_windows() {
        let value = json!({
            "five_hour": {"utilization": 32.5, "resets_at": "2026-07-08T09:00:00Z"},
            "seven_day": {"utilization": 12.0, "resets_at": "2026-07-12T00:00:00Z"},
            "seven_day_opus": null,
        });
        let usage = parse_claude_usage(&value);
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "5h");
        assert!((usage.windows[0].used_percent - 32.5).abs() < f64::EPSILON);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].label, "week");
    }

    #[test]
    fn parses_codex_usage_snake_and_camel() {
        let snake = json!({
            "rate_limits": {
                "primary": {"used_percent": 27.0, "window_minutes": 300, "resets_at": 1779571027},
                "secondary": {"used_percent": 9.0, "window_minutes": 10080, "resets_at": 1779971027}
            },
            "plan_type": "pro"
        });
        let usage = parse_codex_usage(&snake);
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].label, "5h");
        assert_eq!(usage.windows[1].label, "week");
        assert_eq!(usage.plan.as_deref(), Some("pro"));

        let camel = json!({
            "rateLimits": {
                "primary": {"usedPercent": 50.0, "windowDurationMins": 300, "resetsAt": 1779571027}
            }
        });
        let usage = parse_codex_usage(&camel);
        assert_eq!(usage.windows.len(), 1);
        assert!((usage.windows[0].used_percent - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn formats_remaining_percent() {
        let usage = ProviderUsage {
            plan: None,
            windows: vec![UsageWindow {
                label: "5h".to_string(),
                used_percent: 40.0,
                resets_at: None,
            }],
        };
        let line = format_usage(&usage);
        assert!(line.contains("40% used"), "{line}");
        assert!(line.contains("60% left"), "{line}");
    }
}
