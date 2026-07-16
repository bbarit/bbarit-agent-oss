//! Specialist personas the agent can fully adopt (`/persona`). Each persona is
//! a markdown file (frontmatter: name/description/emoji + a full personality
//! brief) under a division directory. Adopting one splices the whole brief
//! into the system prompt so the agent works *as* that specialist — while a
//! fixed adapter keeps bbarit's operating rules (tools, in-place edits,
//! verification) in force. Sub-agents can be given a persona too, so a team
//! can fan out as e.g. code-reviewer + api-tester + ui-designer in parallel.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct Persona {
    /// File stem, used as the stable id (e.g. `code-reviewer`).
    pub id: String,
    /// Division = parent directory (e.g. `engineering`).
    pub division: String,
    pub name: String,
    pub description: String,
    pub emoji: String,
    /// The full personality brief (markdown body, frontmatter stripped).
    pub body: String,
}

/// Operating rules appended after the persona brief: the persona changes HOW
/// the agent works, never WHETHER the harness rules apply.
pub const PERSONA_ADAPTER: &str = "\
You are FULLY in character as this persona for the rest of the session — its \
expertise, standards, priorities, and voice shape every decision and reply. \
The persona changes HOW you work, not the ground rules: keep using the tools \
(read/patch/edit/bash/todo/wiki), keep changes to existing files in place, and \
keep verifying with real runs before declaring anything done.";

fn parse_front_matter(raw: &str) -> (Vec<(String, String)>, String) {
    let Some(rest) = raw.strip_prefix("---") else {
        return (Vec::new(), raw.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (Vec::new(), raw.to_string());
    };
    let header = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
    let fields = header
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    (fields, body)
}

/// Locate the persona library: explicit env override, per-project, per-user,
/// then the development tree (crate directory).
fn personas_roots(config: &AppConfig) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("BBARIT_PERSONAS_DIR") {
        roots.push(PathBuf::from(dir));
    }
    // A persona brief is spliced verbatim into the system prompt, so the
    // project-local dir is a prompt-injection channel — only honor it when the
    // project is trusted (the user dir and bundled library are always safe).
    if config.project_trusted {
        roots.push(config.app_dir.join("personas"));
    }
    roots.push(config.user_app_dir.join("personas"));
    // Installed builds: bundled resources land near the executable — parent
    // paths are mapped under an "_up_" directory, sometimes under "resources".
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().skip(1).take(5) {
            roots.push(ancestor.join("personas"));
            roots.push(ancestor.join("bbarit-agent").join("personas"));
            roots.push(ancestor.join("_up_").join("bbarit-agent").join("personas"));
            roots.push(
                ancestor
                    .join("resources")
                    .join("_up_")
                    .join("bbarit-agent")
                    .join("personas"),
            );
        }
    }
    // Development tree.
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("personas"));
    roots
}

pub fn load_personas(config: &AppConfig) -> Vec<Persona> {
    let Some(root) = personas_roots(config).into_iter().find(|p| p.is_dir()) else {
        return Vec::new();
    };
    let mut personas = Vec::new();
    let Ok(divisions) = std::fs::read_dir(&root) else {
        return personas;
    };
    for division in divisions.flatten() {
        let division_path = division.path();
        if !division_path.is_dir() {
            continue;
        }
        let division_name = division.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(&division_path) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (fields, body) = parse_front_matter(&raw);
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = get("name");
            if id.is_empty() || name.is_empty() || body.trim().is_empty() {
                continue;
            }
            personas.push(Persona {
                id,
                division: division_name.clone(),
                name,
                description: get("description"),
                emoji: get("emoji"),
                body,
            });
        }
    }
    // Engineering group first, the rest alphabetical.
    let rank = |p: &Persona| u8::from(p.division != "engineering");
    personas.sort_by(|a, b| (rank(a), &a.division, &a.id).cmp(&(rank(b), &b.division, &b.id)));
    personas
}

/// Find one persona by id (exact), name (case-insensitive), or substring.
pub fn find_persona(config: &AppConfig, query: &str) -> Result<Persona, String> {
    let query = query.trim();
    let personas = load_personas(config);
    if personas.is_empty() {
        return Err("No persona library found.".to_string());
    }
    if let Some(p) = personas.iter().find(|p| p.id == query) {
        return Ok(p.clone());
    }
    if let Some(p) = personas.iter().find(|p| p.name.eq_ignore_ascii_case(query)) {
        return Ok(p.clone());
    }
    let lowered = query.to_lowercase();
    let matches: Vec<&Persona> = personas
        .iter()
        .filter(|p| {
            p.id.to_lowercase().contains(&lowered)
                || p.name.to_lowercase().contains(&lowered)
                || p.description.to_lowercase().contains(&lowered)
        })
        .collect();
    match matches.len() {
        0 => Err(format!(
            "No persona matches {query:?}. Run /persona to list them."
        )),
        1 => Ok(matches[0].clone()),
        _ => Err(format!(
            "{} personas match {query:?}: {} — be more specific.",
            matches.len(),
            matches
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn active_persona_slot() -> &'static Mutex<Option<Persona>> {
    static ACTIVE: OnceLock<Mutex<Option<Persona>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

pub fn adopt(persona: Persona) {
    *active_persona_slot().lock().unwrap() = Some(persona);
}

pub fn clear() -> bool {
    active_persona_slot().lock().unwrap().take().is_some()
}

/// The persona in effect: explicitly adopted, or requested via the
/// `BBARIT_PERSONA` env (how sub-agents inherit an assignment).
pub fn effective_persona(config: &AppConfig) -> Option<Persona> {
    if let Some(active) = active_persona_slot().lock().unwrap().clone() {
        return Some(active);
    }
    let requested = std::env::var("BBARIT_PERSONA").ok()?;
    find_persona(config, &requested).ok()
}

/// A persona brief may pin an operating mode with a `%%mode=<mode>` line
/// anywhere in its body. `readonly` makes the harness skip mutating tools
/// while the persona is active (a reviewer/advisor persona can then never
/// "helpfully" edit code). The directive is a body line, not frontmatter, so
/// custom persona files stay plain markdown.
pub(crate) fn body_mode(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        line.trim()
            .strip_prefix("%%mode=")
            .map(|mode| mode.trim().to_lowercase())
    })
}

/// True while the active persona declares `%%mode=readonly`.
pub fn persona_is_readonly(config: &AppConfig) -> bool {
    effective_persona(config)
        .map(|persona| body_mode(&persona.body).as_deref() == Some("readonly"))
        .unwrap_or(false)
}

/// The persona body with any `%%mode=` directive lines removed — the mode is
/// harness policy, not prose the model should see verbatim.
pub fn strip_mode_directive(body: &str) -> String {
    body.lines()
        .filter(|line| !line.trim().starts_with("%%mode="))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the persona list for `/persona` with no arguments.
pub fn render_list(config: &AppConfig) -> String {
    let personas = load_personas(config);
    if personas.is_empty() {
        return "No persona library found (expected a `personas/` directory).".to_string();
    }
    let active = active_persona_slot().lock().unwrap().clone();
    let mut out = Vec::new();
    if let Some(active) = &active {
        out.push(format!(
            "Active persona: {} {} — /persona off to drop it.",
            active.emoji, active.name
        ));
    }
    out.push(format!("Personas ({}):", personas.len()));
    let mut division = String::new();
    for p in &personas {
        if p.division != division {
            division = p.division.clone();
            out.push(format!("[{division}]"));
        }
        let mut summary: String = p.description.chars().take(72).collect();
        if p.description.chars().count() > 72 {
            summary.push('…');
        }
        out.push(format!("  {} {:<28} {}", p.emoji, p.id, summary));
    }
    out.push("Usage: /persona <id> to adopt one · /persona off to drop it. Sub-agents: task/agent_team accept a `persona` argument.".to_string());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_directive_is_parsed_and_stripped() {
        let body = "You are a strict code reviewer.\n%%mode=readonly\nFocus on bugs.";
        assert_eq!(body_mode(body).as_deref(), Some("readonly"));
        let stripped = strip_mode_directive(body);
        assert!(!stripped.contains("%%mode"), "{stripped}");
        assert!(stripped.contains("strict code reviewer"));
        assert!(stripped.contains("Focus on bugs"));
        // No directive -> no mode, body unchanged.
        assert_eq!(body_mode("plain persona"), None);
        assert_eq!(strip_mode_directive("plain persona"), "plain persona");
    }

    fn test_config() -> AppConfig {
        let dir = std::env::temp_dir().join("bbarit-personas-test");
        let _ = std::fs::create_dir_all(&dir);
        AppConfig::for_test(dir)
    }

    #[test]
    fn library_loads_curated_personas() {
        let personas = load_personas(&test_config());
        assert!(
            personas.len() >= 25,
            "expected curated library, got {}",
            personas.len()
        );
        let reviewer = personas
            .iter()
            .find(|p| p.id == "code-reviewer")
            .expect("code-reviewer exists");
        assert_eq!(reviewer.division, "engineering");
        assert!(!reviewer.name.is_empty());
        assert!(
            reviewer.body.contains("You are"),
            "body carries the personality brief"
        );
    }

    #[test]
    fn find_matches_id_name_and_substring() {
        let config = test_config();
        assert_eq!(
            find_persona(&config, "code-reviewer").unwrap().id,
            "code-reviewer"
        );
        // Name, case-insensitive.
        let byname = find_persona(&config, "Code Reviewer");
        assert!(byname.is_ok());
        // Ambiguous substring lists candidates.
        let error = find_persona(&config, "e").unwrap_err();
        assert!(error.contains("be more specific"), "{error}");
        // None: no match.
        assert!(find_persona(&config, "zzz-not-real").is_err());
    }

    #[test]
    fn adopt_and_clear_roundtrip() {
        let config = test_config();
        let persona = find_persona(&config, "code-reviewer").unwrap();
        adopt(persona);
        assert_eq!(effective_persona(&config).unwrap().id, "code-reviewer");
        assert!(clear());
        assert!(!clear(), "second clear is a no-op");
    }

    #[test]
    fn front_matter_parsing_boundaries() {
        // Normal.
        let (fields, body) = parse_front_matter("---\nname: X\ndescription: d\n---\n\nBody");
        assert_eq!(fields.iter().find(|(k, _)| k == "name").unwrap().1, "X");
        assert_eq!(body, "Body");
        // None: no frontmatter → whole text is body.
        let (fields, body) = parse_front_matter("Just body");
        assert!(fields.is_empty());
        assert_eq!(body, "Just body");
        // Malformed: unterminated frontmatter → treated as body.
        let (fields, _) = parse_front_matter("---\nname: X\nno end");
        assert!(fields.is_empty());
    }
}
