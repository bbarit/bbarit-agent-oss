use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::{
    APP_DIR, AppConfig, is_git_package_source, is_local_package_source, is_npm_package_source,
    package_storage_base, resolved_package_root,
};

pub fn handle(cli: &Cli, config: &AppConfig) -> Result<Option<String>> {
    let Some(command) = cli.inputs.first().map(String::as_str) else {
        return Ok(None);
    };
    match command {
        "list" => list_packages(config).map(Some),
        "install" => {
            let source = package_source(cli)?;
            add_package(config, source, cli.local).map(Some)
        }
        "remove" | "uninstall" => {
            let source = package_source(cli)?;
            remove_package(config, source, cli.local).map(Some)
        }
        "update" => update_packages(config, cli).map(Some),
        _ => Ok(None),
    }
}

fn package_source(cli: &Cli) -> Result<&str> {
    cli.inputs
        .get(1)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("usage: bbarit install|remove <source> [-l]"))
}

fn list_packages(config: &AppConfig) -> Result<String> {
    let mut lines = Vec::new();
    append_package_lines(
        &mut lines,
        "user",
        &config.user_app_dir.join("settings.json"),
    )?;
    if config.project_trusted {
        append_package_lines(&mut lines, "project", &config.app_dir.join("settings.json"))?;
    } else if config.project_resources_detected {
        lines.push("project\t<not trusted>".to_string());
    }
    if lines.is_empty() {
        Ok("No packages configured.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn append_package_lines(lines: &mut Vec<String>, scope: &str, path: &Path) -> Result<()> {
    let settings = read_settings_value(path)?;
    let Some(packages) = settings.get("packages").and_then(Value::as_array) else {
        return Ok(());
    };
    for package in packages {
        lines.push(format!("{scope}\t{}", package_label(package)));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct ConfiguredPackage {
    source: String,
    local: bool,
}

fn configured_packages(config: &AppConfig) -> Result<Vec<ConfiguredPackage>> {
    let mut packages = Vec::new();
    append_configured_packages(
        &mut packages,
        false,
        &config.user_app_dir.join("settings.json"),
    )?;
    if config.project_trusted {
        append_configured_packages(&mut packages, true, &config.app_dir.join("settings.json"))?;
    }
    Ok(packages)
}

fn append_configured_packages(
    out: &mut Vec<ConfiguredPackage>,
    local: bool,
    path: &Path,
) -> Result<()> {
    let settings = read_settings_value(path)?;
    let Some(packages) = settings.get("packages").and_then(Value::as_array) else {
        return Ok(());
    };
    for package in packages {
        if let Some(source) = package_source_from_value(package) {
            out.push(ConfiguredPackage { source, local });
        }
    }
    Ok(())
}

fn package_source_from_value(value: &Value) -> Option<String> {
    value.as_str().map(ToOwned::to_owned).or_else(|| {
        value
            .get("source")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn package_label(value: &Value) -> String {
    if let Some(source) = value.as_str() {
        return source.to_string();
    }
    if let Some(source) = value.get("source").and_then(Value::as_str) {
        let mut filters = Vec::new();
        for key in ["extensions", "skills", "prompts", "themes"] {
            if value.get(key).is_some() {
                filters.push(key);
            }
        }
        if filters.is_empty() {
            source.to_string()
        } else {
            format!("{source}\tfilters: {}", filters.join(","))
        }
    } else {
        value.to_string()
    }
}

fn add_package(config: &AppConfig, source: &str, local: bool) -> Result<String> {
    let path = settings_path(config, local)?;
    let install_message = install_package_source(config, source, local)?;
    let mut settings = read_settings_value(&path)?;
    let packages = ensure_packages_array(&mut settings)?;
    if packages.iter().any(|entry| package_matches(entry, source)) {
        return Ok(format!(
            "Package already configured in {} settings: {source}",
            scope_label(local)
        ));
    }
    packages.push(json!(source));
    write_settings_value(&path, &settings)?;
    let mut message = format!("Added package to {} settings: {source}", scope_label(local));
    if let Some(install_message) = install_message {
        message.push('\n');
        message.push_str(&install_message);
    }
    Ok(message)
}

fn remove_package(config: &AppConfig, source: &str, local: bool) -> Result<String> {
    let path = settings_path(config, local)?;
    let mut settings = read_settings_value(&path)?;
    let Some(packages) = settings.get_mut("packages").and_then(Value::as_array_mut) else {
        return Ok(format!(
            "No package matched in {} settings: {source}",
            scope_label(local)
        ));
    };
    let before = packages.len();
    packages.retain(|entry| !package_matches(entry, source));
    if packages.len() == before {
        return Ok(format!(
            "No package matched in {} settings: {source}",
            scope_label(local)
        ));
    }
    write_settings_value(&path, &settings)?;
    let removed_install = remove_installed_package(config, source, local)?;
    let mut message = format!(
        "Removed package from {} settings: {source}",
        scope_label(local)
    );
    if let Some(path) = removed_install {
        message.push('\n');
        message.push_str(&format!("Removed installed package: {}", path.display()));
    }
    Ok(message)
}

fn update_packages(config: &AppConfig, cli: &Cli) -> Result<String> {
    if cli.update_self && !cli.all && !cli.update_extensions {
        return Ok(
            "Self update is not implemented in this Rust port yet. Use update --extensions or update <source> for packages."
                .to_string(),
        );
    }

    let requested_source = cli
        .inputs
        .get(1)
        .filter(|value| !value.starts_with('-'))
        .map(String::as_str);
    let mut packages = configured_packages(config)?;
    if let Some(source) = requested_source {
        packages.retain(|package| package.source == source);
        if packages.is_empty() {
            bail!("No configured package matched: {source}");
        }
    } else if !cli.all && !cli.update_extensions {
        return Ok(
            "Self update is not implemented in this Rust port yet. Use update --extensions, update --all, or update <source> for packages."
                .to_string(),
        );
    }

    let mut lines = Vec::new();
    for package in packages {
        match update_package_source(config, &package.source, package.local) {
            Ok(Some(message)) => lines.push(message),
            Ok(None) => lines.push(format!("Skipped local package: {}", package.source)),
            Err(error) => lines.push(format!("Failed to update {}: {error:#}", package.source)),
        }
    }
    if lines.is_empty() {
        Ok("No packages configured.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn settings_path(config: &AppConfig, local: bool) -> Result<PathBuf> {
    if local && !config.project_trusted {
        bail!("This project is untrusted — pass --approve to change its local package config.");
    }
    Ok(if local {
        config.cwd.join(APP_DIR).join("settings.json")
    } else {
        config.user_app_dir.join("settings.json")
    })
}

fn scope_label(local: bool) -> &'static str {
    if local { "project" } else { "user" }
}

fn read_settings_value(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let raw = raw.trim_start_matches('\u{feff}');
    serde_json::from_str(raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_settings_value(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_packages_array(settings: &mut Value) -> Result<&mut Vec<Value>> {
    if !settings.is_object() {
        *settings = json!({});
    }
    let object = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings root must be a JSON object"))?;
    if !object
        .get("packages")
        .is_some_and(|packages| packages.is_array())
    {
        object.insert("packages".to_string(), json!([]));
    }
    object
        .get_mut("packages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("settings.packages must be an array"))
}

fn package_matches(entry: &Value, source: &str) -> bool {
    entry.as_str() == Some(source)
        || entry
            .get("source")
            .and_then(Value::as_str)
            .is_some_and(|value| value == source)
}

fn install_package_source(config: &AppConfig, source: &str, local: bool) -> Result<Option<String>> {
    if is_local_package_source(source) {
        return Ok(None);
    }
    if is_npm_package_source(source) {
        let spec = source.trim().trim_start_matches("npm:");
        install_npm_package(config, spec, local)?;
        let installed = resolved_package_root(&package_scope_base(config, local), source);
        return Ok(Some(format!(
            "Installed npm package: {}",
            installed.display()
        )));
    }
    if is_git_package_source(source) {
        let installed = install_git_package(config, source, local)?;
        return Ok(Some(format!(
            "Installed git package: {}",
            installed.display()
        )));
    }
    Ok(None)
}

fn update_package_source(config: &AppConfig, source: &str, local: bool) -> Result<Option<String>> {
    if is_local_package_source(source) {
        return Ok(None);
    }
    if is_npm_package_source(source) {
        let spec = source.trim().trim_start_matches("npm:");
        install_npm_package(config, spec, local)?;
        let installed = resolved_package_root(&package_scope_base(config, local), source);
        return Ok(Some(format!(
            "Updated npm package: {}",
            installed.display()
        )));
    }
    if is_git_package_source(source) {
        let installed = install_git_package(config, source, local)?;
        return Ok(Some(format!(
            "Updated git package: {}",
            installed.display()
        )));
    }
    Ok(None)
}

fn remove_installed_package(
    config: &AppConfig,
    source: &str,
    local: bool,
) -> Result<Option<PathBuf>> {
    if is_local_package_source(source) {
        return Ok(None);
    }
    if is_npm_package_source(source) {
        let spec = source.trim().trim_start_matches("npm:");
        uninstall_npm_package(config, spec, local)?;
    }
    let installed = resolved_package_root(&package_scope_base(config, local), source);
    if installed.exists() {
        fs::remove_dir_all(&installed)
            .with_context(|| format!("failed to remove {}", installed.display()))?;
        return Ok(Some(installed));
    }
    Ok(None)
}

fn package_scope_base(config: &AppConfig, local: bool) -> PathBuf {
    if local {
        config.app_dir.clone()
    } else {
        config.user_app_dir.clone()
    }
}

fn install_npm_package(config: &AppConfig, spec: &str, local: bool) -> Result<()> {
    let base = package_scope_base(config, local);
    let npm_root = package_storage_base(&base).join("npm");
    fs::create_dir_all(&npm_root)
        .with_context(|| format!("failed to create {}", npm_root.display()))?;
    let (command, mut args, _) = npm_command(config)?;
    args.extend(npm_install_args(config, spec, &npm_root));
    let status = crate::spawn::no_window_command(&command)
        .args(&args)
        .status()
        .with_context(|| format!("failed to run {command}"))?;
    if !status.success() {
        bail!("npm install failed for {spec} with {status}");
    }
    Ok(())
}

fn uninstall_npm_package(config: &AppConfig, spec: &str, local: bool) -> Result<()> {
    let base = package_scope_base(config, local);
    let npm_root = package_storage_base(&base).join("npm");
    if !npm_root.exists() {
        return Ok(());
    }
    let package_name = npm_uninstall_name(spec);
    let (command, mut args, _) = npm_command(config)?;
    args.extend(npm_uninstall_args(config, &package_name, &npm_root));
    let status = crate::spawn::no_window_command(&command)
        .args(&args)
        .status()
        .with_context(|| format!("failed to run {command}"))?;
    if !status.success() {
        bail!("npm uninstall failed for {package_name} with {status}");
    }
    Ok(())
}

fn install_git_package(config: &AppConfig, source: &str, local: bool) -> Result<PathBuf> {
    let base = package_scope_base(config, local);
    let installed = resolved_package_root(&base, source);
    let parsed = parse_git_source(source);
    if installed.exists() {
        reconcile_git_checkout(&installed, parsed.ref_name.as_deref())?;
    } else {
        if let Some(parent) = installed.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let status = crate::spawn::no_window_command("git")
            .arg("clone")
            .arg(&parsed.repo)
            .arg(&installed)
            .status()
            .with_context(|| "failed to run git clone")?;
        if !status.success() {
            bail!("git clone failed for {} with {status}", parsed.repo);
        }
        if let Some(ref_name) = parsed.ref_name.as_deref() {
            checkout_git_ref(&installed, ref_name)?;
        }
    }
    install_git_dependencies(config, &installed)?;
    Ok(installed)
}

fn reconcile_git_checkout(path: &Path, ref_name: Option<&str>) -> Result<()> {
    run_git(path, ["fetch", "--all", "--prune"])?;
    if let Some(ref_name) = ref_name {
        checkout_git_ref(path, ref_name)?;
        run_git(path, ["reset", "--hard", ref_name])?;
    } else if reset_to_first_existing_ref(path, &["origin/HEAD", "origin/main", "origin/master"])
        .is_err()
    {
        run_git(path, ["pull", "--ff-only"])?;
    }
    run_git(path, ["clean", "-fd"])?;
    Ok(())
}

fn reset_to_first_existing_ref(path: &Path, refs: &[&str]) -> Result<()> {
    for ref_name in refs {
        let exists = crate::spawn::no_window_command("git")
            .args(["rev-parse", "--verify", ref_name])
            .current_dir(path)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if exists {
            return run_git(path, ["reset", "--hard", ref_name]);
        }
    }
    bail!("no fetched ref found for git package update")
}

fn checkout_git_ref(path: &Path, ref_name: &str) -> Result<()> {
    run_git(path, ["checkout", ref_name])
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<()> {
    let status = crate::spawn::no_window_command("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("failed to run git in {}", cwd.display()))?;
    if !status.success() {
        bail!("git command failed in {} with {status}", cwd.display());
    }
    Ok(())
}

fn install_git_dependencies(config: &AppConfig, path: &Path) -> Result<()> {
    let package_json = path.join("package.json");
    if !package_json.exists() || !package_has_runtime_dependencies(&package_json)? {
        return Ok(());
    }
    let (command, mut args, configured) = npm_command(config)?;
    if configured {
        args.push("install".to_string());
    } else {
        args.extend(["install".to_string(), "--omit=dev".to_string()]);
    }
    let status = crate::spawn::no_window_command(&command)
        .args(&args)
        .current_dir(path)
        .status()
        .with_context(|| format!("failed to run {command} in {}", path.display()))?;
    if !status.success() {
        bail!("npm install failed in {} with {status}", path.display());
    }
    Ok(())
}

fn npm_command(config: &AppConfig) -> Result<(String, Vec<String>, bool)> {
    let Some(command) = config
        .npm_command
        .as_ref()
        .filter(|command| !command.is_empty())
    else {
        return Ok(("npm".to_string(), Vec::new(), false));
    };
    let first = command
        .first()
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if first.is_empty() {
        bail!("Invalid npmCommand: the first array entry has to be a non-empty command");
    }
    Ok((
        first.to_string(),
        command.iter().skip(1).cloned().collect(),
        true,
    ))
}

fn npm_install_args(config: &AppConfig, spec: &str, install_root: &Path) -> Vec<String> {
    let install_root = install_root.display().to_string();
    match package_manager_name(config).as_deref() {
        Some("bun") => vec![
            "install".to_string(),
            spec.to_string(),
            "--cwd".to_string(),
            install_root,
            "--omit=peer".to_string(),
        ],
        Some("pnpm") => vec![
            "install".to_string(),
            spec.to_string(),
            "--prefix".to_string(),
            install_root,
            "--config.auto-install-peers=false".to_string(),
            "--config.strict-peer-dependencies=false".to_string(),
            "--config.strict-dep-builds=false".to_string(),
        ],
        _ => vec![
            "install".to_string(),
            spec.to_string(),
            "--prefix".to_string(),
            install_root,
            "--legacy-peer-deps".to_string(),
        ],
    }
}

fn npm_uninstall_args(config: &AppConfig, name: &str, install_root: &Path) -> Vec<String> {
    let install_root = install_root.display().to_string();
    if package_manager_name(config).as_deref() == Some("bun") {
        return vec![
            "uninstall".to_string(),
            name.to_string(),
            "--cwd".to_string(),
            install_root,
        ];
    }
    vec![
        "uninstall".to_string(),
        name.to_string(),
        "--prefix".to_string(),
        install_root,
    ]
}

fn npm_uninstall_name(spec: &str) -> String {
    let spec = spec
        .trim()
        .strip_prefix("file:")
        .or_else(|| spec.trim().strip_prefix("link:"))
        .unwrap_or(spec.trim());
    if spec.starts_with('@') {
        let mut parts = spec.split('@');
        let _ = parts.next();
        return format!("@{}", parts.next().unwrap_or(spec).trim_end_matches('/'));
    }
    spec.split('@')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(spec)
        .to_string()
}

fn package_manager_name(config: &AppConfig) -> Option<String> {
    let mut parts = config
        .npm_command
        .clone()
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| vec!["npm".to_string()]);
    if let Some(separator) = parts.iter().rposition(|part| part == "--") {
        parts = parts.into_iter().skip(separator + 1).collect();
    }
    let command = parts.first()?;
    let file_name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    let lower = file_name.to_ascii_lowercase();
    Some(
        lower
            .trim_end_matches(".cmd")
            .trim_end_matches(".exe")
            .to_string(),
    )
}

fn package_has_runtime_dependencies(path: &Path) -> Result<bool> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok([
        "dependencies",
        "optionalDependencies",
        "bundledDependencies",
    ]
    .iter()
    .any(|key| {
        value.get(key).is_some_and(|field| {
            field.as_object().is_some_and(|object| !object.is_empty())
                || field.as_array().is_some_and(|array| !array.is_empty())
        })
    }))
}

struct ParsedGitSource {
    repo: String,
    ref_name: Option<String>,
}

fn parse_git_source(source: &str) -> ParsedGitSource {
    let value = source.trim().strip_prefix("git:").unwrap_or(source.trim());
    let Some((repo, ref_name)) = value.rsplit_once('@') else {
        return ParsedGitSource {
            repo: normalize_git_repo(value),
            ref_name: None,
        };
    };
    if repo.is_empty() || ref_name.is_empty() || looks_like_unpinned_scp_git_source(repo, ref_name)
    {
        return ParsedGitSource {
            repo: normalize_git_repo(value),
            ref_name: None,
        };
    }
    ParsedGitSource {
        repo: normalize_git_repo(repo),
        ref_name: Some(ref_name.to_string()),
    }
}

fn looks_like_unpinned_scp_git_source(repo: &str, ref_name: &str) -> bool {
    repo == "git" && ref_name.contains(':')
}

fn normalize_git_repo(value: &str) -> String {
    if value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("ssh://")
        || value.starts_with("git://")
        || value.starts_with("git@")
        || Path::new(value).exists()
    {
        value.to_string()
    } else {
        format!("https://{value}")
    }
}
