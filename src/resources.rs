use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::config::{AppConfig, PackageSpec, ResourcePathSpec};

#[derive(Debug, Clone)]
pub struct PromptTemplate {
    pub name: String,
    pub description: String,
    pub content: String,
    pub file_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub file_path: PathBuf,
    pub disable_model_invocation: bool,
}

#[derive(Debug, Deserialize, Default)]
struct PackageJson {
    pi: Option<PiPackageManifest>,
}

#[derive(Debug, Deserialize, Default)]
struct PiPackageManifest {
    prompts: Option<Vec<String>>,
    skills: Option<Vec<String>>,
}

pub fn load_prompts(config: &AppConfig) -> Result<Vec<PromptTemplate>> {
    let mut prompts = Vec::new();
    if !config.no_prompt_templates {
        for path in resource_paths(config, "prompts") {
            load_prompts_from_path(&path, &mut prompts)?;
        }
    }
    for path in configured_resource_paths(&config.prompt_paths, "prompts")? {
        load_prompts_from_path(&path, &mut prompts)?;
    }
    if !config.no_prompt_templates {
        for path in package_resource_paths(config, "prompts")? {
            load_prompts_from_path(&path, &mut prompts)?;
        }
    }
    // First discovered wins on name collision when de-duplicating prompts
    // (and the theme loader's entry/or_insert behavior).
    let mut deduped: BTreeMap<String, PromptTemplate> = BTreeMap::new();
    for prompt in prompts {
        deduped.entry(prompt.name.clone()).or_insert(prompt);
    }
    Ok(deduped.into_values().collect())
}

/// Skills are re-scanned from disk at most once per TTL window: the scan walks
/// every skills directory and reads every SKILL.md, and the system-prompt build
/// asks for it on every turn. `/reload` invalidates explicitly.
const SKILLS_CACHE_TTL: Duration = Duration::from_secs(5);

struct SkillsCacheEntry {
    key: String,
    at: Instant,
    skills: Vec<Skill>,
}

static SKILLS_CACHE: OnceLock<Mutex<Option<SkillsCacheEntry>>> = OnceLock::new();

fn skills_cache() -> &'static Mutex<Option<SkillsCacheEntry>> {
    SKILLS_CACHE.get_or_init(|| Mutex::new(None))
}

fn skills_cache_key(config: &AppConfig) -> String {
    format!(
        "{}|{}|{}|{}|{:?}",
        config.cwd.display(),
        config.user_app_dir.display(),
        config.no_skills,
        config.project_trusted,
        config.skill_paths,
    ) + if crate::mcp::interop_enabled() {
        "|interop"
    } else {
        ""
    }
}

/// Drop the cached skills scan so the next call re-reads from disk.
pub fn invalidate_skills_cache() {
    if let Ok(mut cache) = skills_cache().lock() {
        *cache = None;
    }
}

/// Scaffold a new project skill at `.agents/skills/<name>/SKILL.md` and return
/// the created file. Refuses to overwrite an existing skill.
pub fn scaffold_skill(cwd: &Path, name: &str, description: &str) -> Result<PathBuf> {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err(anyhow!("usage: /skill new <name> [description]"));
    }
    let dir = cwd.join(".agents").join("skills").join(&slug);
    let path = dir.join("SKILL.md");
    if path.exists() {
        return Err(anyhow!("skill already exists: {}", path.display()));
    }
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let description = if description.trim().is_empty() {
        "TODO: one line — when should the agent reach for this skill?"
    } else {
        description.trim()
    };
    fs::write(
        &path,
        format!(
            "---\nname: {slug}\ndescription: {description}\n---\n\n# {slug}\n\n\
             Instructions the agent follows when this skill is invoked.\n\n\
             - Keep steps concrete and verifiable.\n\
             - Relative paths here resolve against this folder.\n"
        ),
    )
    .with_context(|| format!("cannot write {}", path.display()))?;
    invalidate_skills_cache();
    Ok(path)
}

pub fn load_skills(config: &AppConfig) -> Result<Vec<Skill>> {
    let key = skills_cache_key(config);
    if let Ok(cache) = skills_cache().lock()
        && let Some(entry) = cache.as_ref()
        && entry.key == key
        && entry.at.elapsed() < SKILLS_CACHE_TTL
    {
        return Ok(entry.skills.clone());
    }
    let skills = scan_skills(config)?;
    if let Ok(mut cache) = skills_cache().lock() {
        *cache = Some(SkillsCacheEntry {
            key,
            at: Instant::now(),
            skills: skills.clone(),
        });
    }
    Ok(skills)
}

fn scan_skills(config: &AppConfig) -> Result<Vec<Skill>> {
    let mut skills = Vec::new();
    if !config.no_skills {
        for path in resource_paths(config, "skills") {
            load_skills_from_path(&path, true, &mut skills)?;
        }
    }
    for path in configured_resource_paths(&config.skill_paths, "skills")? {
        load_skills_from_path(&path, true, &mut skills)?;
    }
    if !config.no_skills {
        for path in package_resource_paths(config, "skills")? {
            load_skills_from_path(&path, true, &mut skills)?;
        }
    }
    // First discovered wins on name collision when de-duplicating.
    let mut deduped: BTreeMap<String, Skill> = BTreeMap::new();
    for skill in skills {
        deduped.entry(skill.name.clone()).or_insert(skill);
    }
    Ok(deduped.into_values().collect())
}

pub fn expand_prompt(config: &AppConfig, name: &str, args: &str) -> Result<String> {
    let prompt = load_prompts(config)?
        .into_iter()
        .find(|prompt| prompt.name == name)
        .ok_or_else(|| anyhow!("no prompt named {name}"))?;
    let args = parse_command_args(args);
    Ok(substitute_args(&prompt.content, &args))
}

pub fn expand_prompt_command(config: &AppConfig, input: &str) -> Result<Option<String>> {
    let Some(rest) = input.trim().strip_prefix('/') else {
        return Ok(None);
    };
    let (name, args) = split_command(rest);
    if name.is_empty() {
        return Ok(None);
    }
    let Some(prompt) = load_prompts(config)?
        .into_iter()
        .find(|prompt| prompt.name == name)
    else {
        return Ok(None);
    };
    let args = parse_command_args(args);
    Ok(Some(substitute_args(&prompt.content, &args)))
}

pub fn skill_command_invocation(config: &AppConfig, name: &str, args: &str) -> Result<String> {
    let skill = load_skills(config)?
        .into_iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| anyhow!("no skill named {name}"))?;
    let base_dir = skill.file_path.parent().unwrap_or_else(|| Path::new("."));
    let block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {}.\n\n{}\n</skill>",
        skill.name,
        skill.file_path.display(),
        base_dir.display(),
        skill.content
    );
    if args.trim().is_empty() {
        Ok(block)
    } else {
        Ok(format!("{block}\n\n{}", args.trim()))
    }
}

/// Prompt space is billed every turn, so the skills listing is compact: shared
/// base dirs are named once (b0, b1, …) and each skill is one line with its
/// path relative to a base and a whitespace-collapsed, length-clamped
/// description. Full descriptions stay in the skill file itself.
const SKILL_PROMPT_DESCRIPTION_LIMIT: usize = 280;

fn clamp_skill_description(description: &str) -> String {
    let collapsed = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SKILL_PROMPT_DESCRIPTION_LIMIT {
        return collapsed;
    }
    let mut out: String = collapsed
        .chars()
        .take(SKILL_PROMPT_DESCRIPTION_LIMIT)
        .collect();
    out.push('…');
    out
}

pub fn format_skills_for_prompt(config: &AppConfig) -> Result<String> {
    let mut visible = load_skills(config)?
        .into_iter()
        .filter(|skill| !skill.disable_model_invocation)
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return Ok(String::new());
    }
    // Interop/home-scanned third-party skill dirs rank after the user's own
    // and the project's — when the block budget truncates, those drop first.
    let third_party: Vec<PathBuf> = dirs_next::home_dir()
        .map(|home| {
            vec![
                home.join(".agents").join("skills"),
                home.join(".claude").join("skills"),
                home.join(".codex").join("skills"),
            ]
        })
        .unwrap_or_default();
    visible.sort_by_key(|skill| {
        third_party
            .iter()
            .any(|dir| skill.file_path.starts_with(dir)) as usize
    });
    let mut bases: Vec<String> = Vec::new();
    let mut rows: Vec<String> = Vec::new();
    for skill in &visible {
        let path = &skill.file_path;
        let base_dir = if path
            .file_stem()
            .map(|stem| stem.eq_ignore_ascii_case("SKILL"))
            .unwrap_or(false)
        {
            path.parent().and_then(|dir| dir.parent())
        } else {
            path.parent()
        };
        let location = match base_dir {
            Some(base) => {
                let base_str = base.display().to_string();
                let id = bases
                    .iter()
                    .position(|known| *known == base_str)
                    .unwrap_or_else(|| {
                        bases.push(base_str);
                        bases.len() - 1
                    });
                match path.strip_prefix(base) {
                    Ok(rel) => format!("b{id}/{}", rel.display()),
                    Err(_) => path.display().to_string(),
                }
            }
            None => path.display().to_string(),
        };
        rows.push(format!(
            "- {} ({location}): {}",
            skill.name,
            clamp_skill_description(&skill.description)
        ));
    }
    let mut lines = vec![
        String::from(
            "\n\nSkills below carry task-specific instructions you can pull in on demand.",
        ),
        String::from(
            "When a task lines up with a skill's description, open that skill's file with the read tool before starting.",
        ),
        String::from(
            "Skill file paths are relative to the numbered base dirs below. Relative paths inside a skill file are relative to that skill's own folder; expand them to an absolute path before passing them to any tool.",
        ),
        format!(
            "Bases: {}",
            bases
                .iter()
                .enumerate()
                .map(|(id, base)| format!("b{id}={base}"))
                .collect::<Vec<_>>()
                .join("  ")
        ),
        String::new(),
        String::from("<available_skills>"),
    ];
    // Even one-line rows add up when many third-party skill dirs are scanned —
    // budget the whole block and point at /skills for the tail.
    const SKILLS_PROMPT_BUDGET: usize = 8_000;
    let total = rows.len();
    let mut used = 0usize;
    let mut kept: Vec<String> = Vec::new();
    for row in rows {
        used += row.len() + 1;
        if used > SKILLS_PROMPT_BUDGET && !kept.is_empty() {
            kept.push(format!(
                "- …and {} more skills — run /skills to list them all",
                total - kept.len()
            ));
            break;
        }
        kept.push(row);
    }
    lines.extend(kept);
    lines.push(String::from("</available_skills>"));
    Ok(lines.join("\n"))
}

fn resource_paths(config: &AppConfig, kind: &str) -> Vec<PathBuf> {
    let mut paths = vec![config.user_app_dir.join(kind)];
    if kind == "skills"
        && let Some(home) = dirs_next::home_dir()
    {
        paths.push(home.join(".agents").join("skills"));
        // Claude Code / Codex interop: their skill libraries load as-is —
        // whatever THIS user has installed there. Gated by /interop.
        if crate::mcp::interop_enabled() {
            paths.push(home.join(".claude").join("skills"));
            paths.push(home.join(".codex").join("skills"));
            if config.project_trusted {
                paths.push(config.cwd.join(".claude").join("skills"));
            }
        }
    }
    if config.project_trusted {
        paths.push(config.app_dir.join(kind));
    }
    if kind == "skills" && config.project_trusted {
        paths.extend(ancestor_agents_skill_dirs(&config.cwd));
    }
    // Bundled skill library ships with the agent, mirroring how personas are
    // discovered: near the executable in installed builds, in the dev tree
    // otherwise. Always safe — it's part of the binary distribution.
    if kind == "skills" {
        if let Ok(exe) = std::env::current_exe() {
            for ancestor in exe.ancestors().skip(1).take(5) {
                paths.push(ancestor.join("skills"));
                paths.push(ancestor.join("bbarit-agent").join("skills"));
                paths.push(ancestor.join("_up_").join("bbarit-agent").join("skills"));
                paths.push(
                    ancestor
                        .join("resources")
                        .join("_up_")
                        .join("bbarit-agent")
                        .join("skills"),
                );
            }
        }
        paths.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills"));
    }
    if let Ok(extension_dirs) = crate::extensions::resource_dirs(config, kind) {
        paths.extend(extension_dirs);
    }
    paths
}

fn configured_resource_paths(specs: &[ResourcePathSpec], kind: &str) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < specs.len() {
        let base_dir = specs[index].base_dir.clone();
        let start = index;
        while index < specs.len() && specs[index].base_dir == base_dir {
            index += 1;
        }
        append_configured_resource_group(&specs[start..index], kind, &mut out)?;
    }
    Ok(out)
}

fn package_resource_paths(config: &AppConfig, kind: &str) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for package in &config.packages {
        append_package_resource_paths(package, kind, &mut paths)?;
    }
    Ok(paths)
}

fn append_package_resource_paths(
    package: &PackageSpec,
    kind: &str,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let root = package.resolved_root();
    if !root.exists() {
        return Ok(());
    }
    let filter = match kind {
        "prompts" => package.prompts.as_ref(),
        "skills" => package.skills.as_ref(),
        _ => None,
    };
    if let Some(filter) = filter {
        if filter.is_empty() {
            return Ok(());
        }
        let all_files = package_manifest_or_conventional_files(&root, kind)?;
        out.extend(apply_resource_patterns(
            all_files,
            filter.iter().map(String::as_str),
            &root,
        ));
        return Ok(());
    }

    if let Some(entries) = package_manifest_entries(&root, kind)? {
        out.extend(package_manifest_files_from_entries(&root, &entries, kind)?);
    } else {
        out.push(root.join(kind));
    }
    Ok(())
}

fn package_manifest_or_conventional_files(root: &Path, kind: &str) -> Result<Vec<PathBuf>> {
    if let Some(entries) = package_manifest_entries(root, kind)? {
        package_manifest_files_from_entries(root, &entries, kind)
    } else {
        let mut files = Vec::new();
        collect_resource_files_from_path(&root.join(kind), kind, &mut files)?;
        Ok(files)
    }
}

fn package_manifest_entries(root: &Path, kind: &str) -> Result<Option<Vec<String>>> {
    let manifest_path = root.join("package.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let package: PackageJson = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    Ok(package.pi.and_then(|manifest| match kind {
        "prompts" => manifest.prompts,
        "skills" => manifest.skills,
        _ => None,
    }))
}

fn package_manifest_files_from_entries(
    root: &Path,
    entries: &[String],
    kind: &str,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| !is_resource_override_pattern(entry))
    {
        if entry.contains('*') || entry.contains('?') {
            let mut candidates = Vec::new();
            collect_resource_files_from_path(root, kind, &mut candidates)?;
            files.extend(
                candidates
                    .into_iter()
                    .filter(|path| matches_resource_pattern(path, entry, root)),
            );
        } else {
            collect_resource_files_from_path(&root.join(entry), kind, &mut files)?;
        }
    }
    let patterns = entries
        .iter()
        .filter(|entry| is_resource_override_pattern(entry))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if patterns.is_empty() {
        Ok(files)
    } else {
        Ok(apply_resource_patterns(files, patterns, root))
    }
}

fn is_resource_override_pattern(value: &str) -> bool {
    value.starts_with('!') || value.starts_with('+') || value.starts_with('-')
}

fn append_configured_resource_group(
    specs: &[ResourcePathSpec],
    kind: &str,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    if !specs.iter().any(|spec| is_resource_pattern(&spec.value)) {
        out.extend(specs.iter().map(ResourcePathSpec::resolved_path));
        return Ok(());
    }

    let mut all_files = Vec::new();
    for spec in specs
        .iter()
        .filter(|spec| !is_resource_pattern(&spec.value))
    {
        collect_resource_files_from_path(&spec.resolved_path(), kind, &mut all_files)?;
    }
    out.extend(apply_resource_patterns(
        all_files,
        specs
            .iter()
            .filter(|spec| is_resource_pattern(&spec.value))
            .map(|spec| spec.value.as_str()),
        &specs[0].base_dir,
    ));
    Ok(())
}

fn is_resource_pattern(value: &str) -> bool {
    value.starts_with('!')
        || value.starts_with('+')
        || value.starts_with('-')
        || value.contains('*')
        || value.contains('?')
}

fn collect_resource_files_from_path(path: &Path, kind: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    collect_resource_files_from_dir(path, kind, out)
}

fn collect_resource_files_from_dir(dir: &Path, kind: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    if kind == "skills" {
        let skill_file = dir.join("SKILL.md");
        if skill_file.exists() {
            out.push(skill_file);
            return Ok(());
        }
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            collect_resource_files_from_dir(&path, kind, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn apply_resource_patterns<'a>(
    all_files: Vec<PathBuf>,
    patterns: impl IntoIterator<Item = &'a str>,
    base_dir: &Path,
) -> Vec<PathBuf> {
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    let mut force_includes = Vec::new();
    let mut force_excludes = Vec::new();
    for pattern in patterns {
        if let Some(rest) = pattern.strip_prefix('!') {
            excludes.push(rest);
        } else if let Some(rest) = pattern.strip_prefix('+') {
            force_includes.push(rest);
        } else if let Some(rest) = pattern.strip_prefix('-') {
            force_excludes.push(rest);
        } else {
            includes.push(pattern);
        }
    }

    let mut selected = if includes.is_empty() {
        all_files.clone()
    } else {
        all_files
            .iter()
            .filter(|path| matches_any_pattern(path, &includes, base_dir))
            .cloned()
            .collect()
    };

    if !excludes.is_empty() {
        selected.retain(|path| !matches_any_pattern(path, &excludes, base_dir));
    }
    for path in &all_files {
        if !selected.contains(path) && matches_any_exact(path, &force_includes, base_dir) {
            selected.push(path.clone());
        }
    }
    if !force_excludes.is_empty() {
        selected.retain(|path| !matches_any_exact(path, &force_excludes, base_dir));
    }
    selected
}

fn matches_any_pattern(path: &Path, patterns: &[&str], base_dir: &Path) -> bool {
    patterns
        .iter()
        .any(|pattern| matches_resource_pattern(path, pattern, base_dir))
}

fn matches_resource_pattern(path: &Path, pattern: &str, base_dir: &Path) -> bool {
    let pattern = normalize_pattern(pattern);
    let rel = path_relative_slash(path, base_dir);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    wildcard_match(&pattern, &rel)
        || wildcard_match(&pattern, &file_name)
        || skill_parent_relative(path, base_dir)
            .is_some_and(|parent| wildcard_match(&pattern, &parent))
}

fn matches_any_exact(path: &Path, patterns: &[&str], base_dir: &Path) -> bool {
    patterns
        .iter()
        .map(|pattern| resolve_pattern_path(base_dir, pattern))
        .any(|target| path == target)
}

fn resolve_pattern_path(base_dir: &Path, pattern: &str) -> PathBuf {
    let path = PathBuf::from(pattern);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn normalize_pattern(pattern: &str) -> String {
    pattern
        .trim_start_matches('/')
        .replace('\\', "/")
        .trim()
        .to_string()
}

fn path_relative_slash(path: &Path, base_dir: &Path) -> String {
    path.strip_prefix(base_dir)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn skill_parent_relative(path: &Path, base_dir: &Path) -> Option<String> {
    if path.file_name().and_then(|value| value.to_str()) != Some("SKILL.md") {
        return None;
    }
    path.parent()
        .map(|parent| path_relative_slash(parent, base_dir))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut dp = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    dp[0][0] = true;
    for i in 1..=pattern.len() {
        if pattern[i - 1] == b'*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=pattern.len() {
        for j in 1..=value.len() {
            dp[i][j] = match pattern[i - 1] {
                b'*' => dp[i - 1][j] || dp[i][j - 1],
                b'?' => dp[i - 1][j - 1],
                ch => ch == value[j - 1] && dp[i - 1][j - 1],
            };
        }
    }
    dp[pattern.len()][value.len()]
}

fn ancestor_agents_skill_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let git_root = find_git_root(cwd);
    let user_agents_skills = dirs_next::home_dir().map(|home| home.join(".agents").join("skills"));
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let candidate = dir.join(".agents").join("skills");
        if user_agents_skills.as_ref() != Some(&candidate) {
            paths.push(candidate);
        }
        if git_root.as_deref() == Some(dir) {
            break;
        }
        current = dir.parent();
    }
    paths
}

fn find_git_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = Some(cwd);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn load_prompts_from_path(path: &Path, out: &mut Vec<PromptTemplate>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        load_prompt_file(path, out)?;
        return Ok(());
    }
    load_prompts_from_dir(path, out)
}

fn load_prompts_from_dir(dir: &Path, out: &mut Vec<PromptTemplate>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry?.path();
        load_prompt_file(&path, out)?;
    }
    Ok(())
}

fn load_prompt_file(path: &Path, out: &mut Vec<PromptTemplate>) -> Result<()> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
        return Ok(());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read prompt {}", path.display()))?;
    let (frontmatter, body) = parse_frontmatter(&raw);
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("prompt")
        .to_string();
    let description = frontmatter
        .get("description")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| first_description_line(&body));
    out.push(PromptTemplate {
        name,
        description,
        content: body,
        file_path: path.to_path_buf(),
    });
    Ok(())
}

fn load_skills_from_path(
    path: &Path,
    include_root_files: bool,
    out: &mut Vec<Skill>,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        if let Some(skill) = load_skill_file(path)? {
            out.push(skill);
        }
        return Ok(());
    }
    load_skills_from_dir(path, include_root_files, out)
}

fn load_skills_from_dir(dir: &Path, include_root_files: bool, out: &mut Vec<Skill>) -> Result<()> {
    let skill_file = dir.join("SKILL.md");
    if skill_file.exists() {
        if let Some(skill) = load_skill_file(&skill_file)? {
            out.push(skill);
        }
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            load_skills_from_dir(&path, false, out)?;
            continue;
        }
        if include_root_files
            && path.extension().and_then(|ext| ext.to_str()) == Some("md")
            && let Some(skill) = load_skill_file(&path)?
        {
            out.push(skill);
        }
    }
    Ok(())
}

fn load_skill_file(path: &Path) -> Result<Option<Skill>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read skill {}", path.display()))?;
    let (frontmatter, body) = parse_frontmatter(&raw);
    let Some(description) = frontmatter
        .get("description")
        .cloned()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let fallback_name = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|value| value.to_str())
        .or_else(|| path.file_stem().and_then(|value| value.to_str()))
        .unwrap_or("skill");
    let name = frontmatter
        .get("name")
        .cloned()
        .unwrap_or_else(|| fallback_name.to_string());
    Ok(Some(Skill {
        name,
        description,
        content: body,
        file_path: path.to_path_buf(),
        disable_model_invocation: frontmatter
            .get("disable-model-invocation")
            .is_some_and(|value| value == "true"),
    }))
}

fn parse_frontmatter(raw: &str) -> (BTreeMap<String, String>, String) {
    let mut map = BTreeMap::new();
    let raw = raw.trim_start_matches('\u{feff}');
    if !raw.starts_with("---\n") && !raw.starts_with("---\r\n") {
        return (map, raw.to_string());
    }
    let rest = raw
        .strip_prefix("---\r\n")
        .or_else(|| raw.strip_prefix("---\n"))
        .unwrap_or(raw);
    let Some(end) = rest.find("\n---") else {
        return (map, raw.to_string());
    };
    let fm = &rest[..end];
    let body = rest[end..]
        .strip_prefix("\n---\r\n")
        .or_else(|| rest[end..].strip_prefix("\n---\n"))
        .or_else(|| rest[end..].strip_prefix("\n---"))
        .unwrap_or(&rest[end..])
        .to_string();
    for line in fm.lines() {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            );
        }
    }
    (map, body)
}

fn first_description_line(body: &str) -> String {
    body.lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            let mut value = line.trim().chars().take(60).collect::<String>();
            if line.trim().chars().count() > 60 {
                value.push_str("...");
            }
            value
        })
        .unwrap_or_default()
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

fn split_command(input: &str) -> (&str, &str) {
    let input = input.trim();
    match input.find(char::is_whitespace) {
        Some(index) => (&input[..index], input[index..].trim()),
        None => (input, ""),
    }
}

fn substitute_args(content: &str, args: &[String]) -> String {
    let all_args = args.join(" ");
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
        if after_dollar >= content.len() {
            output.push('$');
            index = after_dollar;
            continue;
        }

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
        if let Some(end) = rest.strip_prefix('{').and_then(|value| value.find('}')) {
            let expression = &rest[1..=end];
            if let Some(value) = substitute_braced_arg(expression, args) {
                output.push_str(&value);
                index = after_dollar + end + 2;
                continue;
            }
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
            output.push_str(&value);
            index = after_dollar + digit_len;
            continue;
        }

        output.push('$');
        index = after_dollar;
    }
    output
}

fn substitute_braced_arg(expression: &str, args: &[String]) -> Option<String> {
    if let Some(rest) = expression.strip_prefix("@:") {
        let mut parts = rest.splitn(2, ':');
        let start = parts.next()?.parse::<usize>().ok()?.saturating_sub(1);
        let start = start.min(args.len());
        let values =
            if let Some(length) = parts.next().and_then(|value| value.parse::<usize>().ok()) {
                args[start..]
                    .iter()
                    .take(length)
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                args[start..].to_vec()
            };
        return Some(values.join(" "));
    }

    let (position, default_value) = expression.split_once(":-")?;
    let position = position.parse::<usize>().ok()?;
    let value = args
        .get(position.saturating_sub(1))
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| default_value.to_string());
    Some(value)
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_skill_creates_loadable_skill_and_refuses_overwrite() {
        let dir = std::env::temp_dir().join(format!("bbarit-oss-skillnew-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = scaffold_skill(&dir, "My Deploy Steps!", "run the deploy checklist").unwrap();
        assert!(path.ends_with(".agents/skills/my-deploy-steps/SKILL.md"));

        let mut skills = Vec::new();
        load_skills_from_path(&dir.join(".agents").join("skills"), true, &mut skills).unwrap();
        let skill = skills
            .iter()
            .find(|s| s.name == "my-deploy-steps")
            .expect("scaffolded skill loads");
        assert_eq!(skill.description, "run the deploy checklist");

        assert!(
            scaffold_skill(&dir, "my deploy steps", "").is_err(),
            "must refuse overwrite"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    use super::*;

    fn fixture_config(
        name: &str,
        prompt_paths: Vec<PathBuf>,
        skill_paths: Vec<PathBuf>,
    ) -> AppConfig {
        let root = std::env::temp_dir().join(format!("bbarit-agent-resources-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        AppConfig {
            cwd: root.clone(),
            app_dir: root.join(".pi"),
            user_app_dir: root.join("user-pi"),
            project_trusted: true,
            project_resources_detected: true,
            session_dir: root.join("sessions"),
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
            no_extensions: false,
            no_skills: false,
            no_prompt_templates: false,
            no_context_files: false,
            no_themes: false,
            extension_paths: Vec::new(),
            packages: Vec::new(),
            prompt_paths: resource_specs(&root, prompt_paths),
            skill_paths: resource_specs(&root, skill_paths),
            theme_paths: Vec::new(),
            enable_skill_commands: true,
            shell_path: None,
            shell_command_prefix: None,
            npm_command: None,
            auth_paths: Vec::new(),
            compaction_enabled: true,
            compaction_reserve_tokens: crate::config::DEFAULT_COMPACTION_RESERVE_TOKENS,
            compaction_keep_recent_tokens: crate::config::DEFAULT_COMPACTION_KEEP_RECENT_TOKENS,
            retry_max_retries: crate::config::DEFAULT_RETRY_MAX_RETRIES,
            stream: false,
            review_model: None,
        }
    }

    fn resource_specs(base_dir: &Path, paths: Vec<PathBuf>) -> Vec<ResourcePathSpec> {
        paths
            .into_iter()
            .map(|path| ResourcePathSpec {
                base_dir: base_dir.to_path_buf(),
                value: path.display().to_string(),
            })
            .collect()
    }

    #[test]
    fn prompt_slash_command_expands_template_arguments() {
        let root = std::env::temp_dir().join("bbarit-agent-resources-prompt-command");
        let _ = fs::remove_dir_all(&root);
        let prompts = root.join("prompts");
        fs::create_dir_all(&prompts).unwrap();
        fs::write(
            prompts.join("direct.md"),
            "---\ndescription: Direct prompt\n---\nfirst=$1 all=$ARGUMENTS",
        )
        .unwrap();
        let config = fixture_config("prompt-command-config", vec![prompts], Vec::new());

        let expanded = expand_prompt_command(&config, "/direct one 'two words'")
            .unwrap()
            .unwrap();

        assert_eq!(expanded, "first=one all=one two words");
        assert!(
            expand_prompt_command(&config, "/missing value")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prompt_arguments_support_defaults_and_slices() {
        let output = substitute_args(
            "a=${1:-fallback} b=${2:-fallback} c=${@:2} d=${@:2:1} e=$@",
            &["one".to_string(), "two".to_string(), "three".to_string()],
        );
        assert_eq!(output, "a=one b=two c=two three d=two e=one two three");

        let output = substitute_args("a=${1:-fallback} b=${@:0:2}", &[]);
        assert_eq!(output, "a=fallback b=");
    }

    #[test]
    fn skills_prompt_uses_pi_available_skills_xml() {
        let root = std::env::temp_dir().join("bbarit-agent-resources-skill-prompt");
        let _ = fs::remove_dir_all(&root);
        let skills = root.join("skills");
        fs::create_dir_all(&skills).unwrap();
        fs::write(
            skills.join("xml.md"),
            "---\nname: xml-skill\ndescription: Use <xml> & quotes\n---\nBody",
        )
        .unwrap();
        let deep = skills.join("deep-skill");
        fs::create_dir_all(&deep).unwrap();
        fs::write(
            deep.join("SKILL.md"),
            &format!(
                "---\nname: deep-skill\ndescription: {}\n---\nBody",
                "long ".repeat(120)
            ),
        )
        .unwrap();
        let config = fixture_config("skill-prompt-config", Vec::new(), vec![skills.clone()]);

        let output = format_skills_for_prompt(&config).unwrap();

        assert!(output.contains("<available_skills>"));
        assert!(output.contains("</available_skills>"));
        // Compact one-line rows against a numbered base dir — the base path
        // appears once (in the Bases line), not on every skill.
        assert!(output.contains(&format!("={}", skills.display())));
        let row = output
            .lines()
            .find(|line| line.starts_with("- xml-skill (b"))
            .expect("xml-skill row");
        assert!(row.contains("/xml.md): Use <xml> & quotes"));
        let deep_row = output
            .lines()
            .find(|line| line.starts_with("- deep-skill (b"))
            .expect("deep-skill row");
        assert!(deep_row.contains("/deep-skill/SKILL.md):"));
        assert_eq!(output.matches(&skills.display().to_string()).count(), 1);
        // Long descriptions are clamped, not injected wholesale.
        assert!(output.contains('…'));
        assert!(!output.contains(&"long ".repeat(120)));
        assert!(!output.contains("<location>"));
    }

    #[test]
    fn skills_load_from_project_agents_directory() {
        let config = fixture_config("project-agents-config", Vec::new(), Vec::new());
        let skill_dir = config.cwd.join(".agents").join("skills").join("reviewer");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: agents-reviewer\ndescription: Project agents skill\n---\nReview body",
        )
        .unwrap();

        let skills = load_skills(&config).unwrap();

        assert!(skills.iter().any(|skill| skill.name == "agents-reviewer"));
    }

    #[test]
    fn prompt_paths_support_pi_include_and_exclude_patterns() {
        let config = fixture_config("prompt-pattern-config", Vec::new(), Vec::new());
        let prompts = config.app_dir.join("prompt-pack");
        fs::create_dir_all(&prompts).unwrap();
        fs::write(
            prompts.join("keep.md"),
            "---\ndescription: Keep prompt\n---\nKeep",
        )
        .unwrap();
        fs::write(
            prompts.join("skip.md"),
            "---\ndescription: Skip prompt\n---\nSkip",
        )
        .unwrap();
        fs::write(
            prompts.join("force.md"),
            "---\ndescription: Force prompt\n---\nForce",
        )
        .unwrap();
        let mut config = config;
        config.prompt_paths = vec![
            ResourcePathSpec {
                base_dir: config.app_dir.clone(),
                value: "prompt-pack".to_string(),
            },
            ResourcePathSpec {
                base_dir: config.app_dir.clone(),
                value: "*.md".to_string(),
            },
            ResourcePathSpec {
                base_dir: config.app_dir.clone(),
                value: "!skip.md".to_string(),
            },
            ResourcePathSpec {
                base_dir: config.app_dir.clone(),
                value: "-prompt-pack/force.md".to_string(),
            },
        ];

        let prompts = load_prompts(&config).unwrap();
        let names = prompts
            .into_iter()
            .map(|prompt| prompt.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"keep".to_string()));
        assert!(!names.contains(&"skip".to_string()));
        assert!(!names.contains(&"force".to_string()));
    }
}
