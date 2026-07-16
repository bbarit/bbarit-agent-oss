use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

use crate::config::AppConfig;

pub fn status(config: &AppConfig) -> Result<String> {
    let resource_state = if config.project_resources_detected {
        "project resources detected"
    } else {
        "no project resources requiring trust"
    };
    let decision = current_decision(config)?;
    let effective = is_trusted(config)?;
    Ok(format!(
        "Trust: {}\nEffective: {}\nProject: {}\nStore: {}\nResources: {}",
        decision_label(decision),
        if effective { "trusted" } else { "not trusted" },
        canonical(&config.cwd).display(),
        trust_path(config).display(),
        resource_state
    ))
}

pub fn set(config: &AppConfig, decision: Option<bool>) -> Result<String> {
    let key = canonical(&config.cwd).display().to_string();
    let path = trust_path(config);
    // Lock across read→modify→write so concurrent bbarit processes don't
    // drop each other's trust decisions.
    let _lock = crate::config::lock_state_file(&path);
    let mut data = read_store(&path)?;
    match decision {
        Some(value) => {
            data.insert(key.clone(), value);
            write_store(&path, &data)?;
            let mut message = format!("Trust saved for {key}: {}", decision_label(Some(value)));
            if value {
                // project_trusted is a startup snapshot, so already-skipped
                // resources only appear after a restart.
                message.push_str("\nProject skills, extensions, and hooks load on the next start.");
            }
            Ok(message)
        }
        None => {
            data.remove(&key);
            write_store(&path, &data)?;
            Ok(format!("Trust decision cleared for {key}"))
        }
    }
}

pub fn require_trusted(config: &AppConfig, operation: &str) -> Result<()> {
    if is_trusted(config)? {
        return Ok(());
    }
    // Inside the TUI stdin is in raw mode and owned by the event loop: a
    // read_line() prompt can never receive the answer, and its print!() lands
    // as garbage text over the composer. Fail with instructions instead — the
    // error shows in the transcript and /trust yes unblocks.
    let tui_active = ratatui::crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
    if !tui_active && atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout) {
        return prompt_for_trust(config, operation);
    }
    let project = canonical(&config.cwd);
    anyhow::bail!(
        "project is not trusted; refusing to {operation} in {}. Run /trust yes first, or /trust no to keep blocking project actions.",
        project.display()
    )
}

fn prompt_for_trust(config: &AppConfig, operation: &str) -> Result<()> {
    let project = canonical(&config.cwd);
    print!(
        "Project {} is not trusted. Trust it and allow {}? [y/N] ",
        project.display(),
        operation
    );
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        set(config, Some(true))?;
        return Ok(());
    }
    set(config, Some(false))?;
    anyhow::bail!(
        "project is not trusted; refusing to {operation} in {}",
        project.display()
    )
}
pub fn is_trusted(config: &AppConfig) -> Result<bool> {
    let decision = current_decision(config)?;
    // An explicit "no" always blocks, even in projects with no detected
    // resources — otherwise /trust no is silently ignored there.
    if decision == Some(false) {
        return Ok(false);
    }
    if !config.project_resources_detected {
        return Ok(true);
    }
    if let Some(decision) = decision {
        return Ok(decision);
    }
    Ok(config.project_trusted)
}

fn current_decision(config: &AppConfig) -> Result<Option<bool>> {
    let data = read_store(&trust_path(config))?;
    let mut current = canonical(&config.cwd);
    loop {
        let key = current.display().to_string();
        if let Some(value) = data.get(&key) {
            return Ok(Some(*value));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn trust_path(config: &AppConfig) -> PathBuf {
    config.user_app_dir.join("trust.json")
}

fn read_store(path: &Path) -> Result<BTreeMap<String, bool>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let raw = raw.trim_start_matches('\u{feff}');
    let parsed: Value = match serde_json::from_str(raw) {
        Ok(parsed) => parsed,
        Err(error) => {
            // Corrupt is not absent: back up the evidence and start from an
            // empty store (the trust prompt simply runs again) instead of
            // refusing to operate.
            let backup = path.with_extension("json.corrupt.bak");
            let _ = fs::copy(path, &backup);
            eprintln!(
                "bbarit: {} is invalid JSON ({error}); backed up to {} and starting fresh.",
                path.display(),
                backup.display()
            );
            return Ok(BTreeMap::new());
        }
    };
    let object = parsed
        .as_object()
        .ok_or_else(|| anyhow!("invalid trust store {}, expected object", path.display()))?;
    let mut data = BTreeMap::new();
    for (key, value) in object {
        if let Some(decision) = value.as_bool() {
            data.insert(key.clone(), decision);
        }
    }
    Ok(data)
}

fn write_store(path: &Path, data: &BTreeMap<String, bool>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Atomic: a crash mid-write must not corrupt the trust store.
    crate::tools::atomic_write(
        path,
        format!("{}\n", serde_json::to_string_pretty(data)?).as_bytes(),
    )?;
    Ok(())
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn decision_label(decision: Option<bool>) -> &'static str {
    match decision {
        Some(true) => "trusted",
        Some(false) => "not trusted",
        None => "not set",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn test_config(name: &str) -> AppConfig {
        let dir = std::env::temp_dir().join(format!("bbarit-agent-trust-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        AppConfig::for_test(dir)
    }

    #[test]
    fn explicit_false_blocks_without_project_resources() {
        let config = test_config("explicit-false");
        assert!(!config.project_resources_detected);
        set(&config, Some(false)).unwrap();
        assert!(!is_trusted(&config).unwrap());
    }

    #[test]
    fn no_decision_without_project_resources_stays_trusted() {
        let config = test_config("no-decision");
        assert!(is_trusted(&config).unwrap());
    }

    #[test]
    fn trust_yes_notes_resources_load_on_next_start() {
        let config = test_config("yes-note");
        let message = set(&config, Some(true)).unwrap();
        assert!(message.contains("load on the next start"));
        assert!(is_trusted(&config).unwrap());
        let cleared = set(&config, None).unwrap();
        assert!(!cleared.contains("load on the next start"));
    }
}
