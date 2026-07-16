//! Project wiki: the agent's own knowledge base.
//!
//! Pages are plain markdown files under the agent's note vault
//! (`~/.bbarit-oss/agent/notes`), fully self-contained — wikilinks, tags and
//! all, editable with any editor. Legacy per-project stores (`.bbarit/wiki.db`,
//! `.bbarit/wiki/`, `.pi/wiki/`) are imported once, without clobbering any
//! note that already exists in the vault.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

pub struct Wiki {
    root: PathBuf,
    /// This project's corner of the vault: `<root>/projects/<slug>`. EVERY
    /// agent operation (get/set/list/search/delete) is scoped to it — one
    /// project's knowledge must never leak into another project's prompt
    /// (observed live: one project's overview page injected into a
    /// different project's review turn). The rest of the vault (other
    /// projects, the user's own notes) is invisible to the agent; the desktop
    /// notes app still shows everything.
    project_dir: PathBuf,
}

impl Wiki {
    /// Open the shared note vault, creating it if needed and importing any
    /// legacy per-project wiki pages once.
    pub fn open(app_dir: &Path, cwd: &Path) -> Result<Self> {
        let root = note_vault_dir();
        let project_dir = root.join("projects").join(project_slug(cwd));
        std::fs::create_dir_all(&project_dir)
            .with_context(|| format!("create {}", project_dir.display()))?;
        let wiki = Self { root, project_dir };
        // One-time, non-destructive migration of legacy stores.
        wiki.import_legacy_md(&app_dir.join("wiki"));
        wiki.import_legacy_md(&cwd.join(".pi").join("wiki"));
        wiki.import_legacy_db(&app_dir.join("wiki.db"));
        Ok(wiki)
    }

    /// The shared vault directory (same as the desktop app's notes).
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn import_legacy_md(&self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let (Some(name), Ok(content)) = (
                path.file_stem().and_then(|stem| stem.to_str()),
                std::fs::read_to_string(&path),
            ) {
                let _ = self.set_if_absent(name, &content);
            }
        }
    }

    fn import_legacy_db(&self, db_path: &Path) {
        if !db_path.exists() {
            return;
        }
        let Ok(conn) = rusqlite::Connection::open(db_path) else {
            return;
        };
        let Ok(mut stmt) = conn.prepare("SELECT name, content FROM pages") else {
            return;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) else {
            return;
        };
        for (name, content) in rows.flatten() {
            let _ = self.set_if_absent(&name, &content);
        }
    }

    fn set_if_absent(&self, name: &str, content: &str) -> Result<()> {
        if self.find(name).is_none() {
            std::fs::write(self.path_for(name), content)?;
        }
        Ok(())
    }

    /// Path for a brand-new page (inside this project's corner of the vault).
    fn path_for(&self, name: &str) -> PathBuf {
        self.project_dir.join(format!("{}.md", sanitize(name)))
    }

    /// Locate an existing page by basename — STRICTLY inside this project's
    /// corner. No fallback to the rest of the vault: guessable names like
    /// "overview" would otherwise pull another project's (or the user's own)
    /// notes into this project's context, which is exactly the leak this
    /// scoping exists to kill. The desktop notes app still shows everything.
    fn find(&self, name: &str) -> Option<PathBuf> {
        let target = format!("{}.md", sanitize(name));
        let mut found = None;
        walk_md(&self.project_dir, &mut |path| {
            if found.is_none()
                && path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .map(|f| f.eq_ignore_ascii_case(&target))
                    .unwrap_or(false)
            {
                found = Some(path.to_path_buf());
            }
        });
        found
    }

    pub fn set(&self, name: &str, content: &str) -> Result<()> {
        let path = self.find(name).unwrap_or_else(|| self.path_for(name));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Option<String>> {
        match self.find(name) {
            Some(path) => Ok(Some(std::fs::read_to_string(path)?)),
            None => Ok(None),
        }
    }

    pub fn delete(&self, name: &str) -> Result<bool> {
        match self.find(name) {
            Some(path) => {
                std::fs::remove_file(path)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Delete EVERY page of this project (its corner of the vault only). Other
    /// projects' notes and the rest of the vault are untouched. Returns how many
    /// pages were removed.
    pub fn reset(&self) -> Result<usize> {
        let mut removed = 0;
        let mut victims = Vec::new();
        walk_md(&self.project_dir, &mut |path| {
            victims.push(path.to_path_buf())
        });
        for path in victims {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// (name, updated_at) for every page OF THIS PROJECT, most recently
    /// modified first. Deliberately does not list the rest of the vault —
    /// this feeds the system prompt, and cross-project pages there derail
    /// the agent ("why is it reviewing a different project?").
    pub fn list(&self) -> Result<Vec<(String, String)>> {
        let mut items: Vec<(String, String, SystemTime)> = Vec::new();
        walk_md(&self.project_dir, &mut |path| {
            if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
                let updated = modified(path);
                items.push((name.to_string(), fmt_time(updated), updated));
            }
        });
        items.sort_by(|a, b| b.2.cmp(&a.2));
        Ok(items
            .into_iter()
            .map(|(name, time, _)| (name, time))
            .collect())
    }

    /// (name, first matching line) for THIS PROJECT's pages whose name or
    /// body matches `query`.
    pub fn search(&self, query: &str) -> Result<Vec<(String, String)>> {
        let needle = query.to_lowercase();
        let mut hits: Vec<(String, String, SystemTime)> = Vec::new();
        walk_md(&self.project_dir, &mut |path| {
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return;
            };
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let line = content
                .lines()
                .find(|line| line.to_lowercase().contains(&needle));
            if name.to_lowercase().contains(&needle) || line.is_some() {
                let snippet = line
                    .or_else(|| content.lines().next())
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect::<String>();
                hits.push((name.to_string(), snippet, modified(path)));
            }
        });
        hits.sort_by(|a, b| b.2.cmp(&a.2));
        Ok(hits
            .into_iter()
            .map(|(name, snippet, _)| (name, snippet))
            .collect())
    }
}

/// The agent's note vault: `~/.bbarit-oss/agent/notes`. `BBARIT_NOTE_VAULT_DIR`
/// overrides it so tests never touch the user's real notes.
fn note_vault_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BBARIT_NOTE_VAULT_DIR") {
        return PathBuf::from(dir);
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::config::USER_APP_ROOT)
        .join("agent")
        .join("notes")
}

/// Make a page name safe to use as a flat filename (no path separators).
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Stable per-project folder name: sanitized basename + a short FNV-1a hash
/// of the full path, so two checkouts named `server` don't share a corner.
fn project_slug(cwd: &Path) -> String {
    let base = cwd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_string());
    let normalized = cwd.to_string_lossy().to_lowercase().replace('\\', "/");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{}-{:08x}", sanitize(&base), (hash & 0xffff_ffff) as u32)
}

fn modified(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn fmt_time(time: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = time.into();
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Recursively visit `*.md` files, skipping hidden files/dirs (e.g. the
/// `.notes-cache.json` and any `.git`).
fn walk_md(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_md(&path, visit);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            visit(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_pages_are_project_scoped_with_legacy_get_fallback() {
        let _env_guard = crate::test_support::env_lock();
        let vault = std::env::temp_dir().join("bbarit-wiki-scope-test");
        let _ = std::fs::remove_dir_all(&vault);
        std::fs::create_dir_all(&vault).unwrap();
        unsafe { std::env::set_var("BBARIT_NOTE_VAULT_DIR", &vault) };

        let project_a = std::env::temp_dir().join("bbarit-wiki-proj-a");
        let project_b = std::env::temp_dir().join("bbarit-wiki-proj-b");
        let _ = std::fs::create_dir_all(&project_a);
        let _ = std::fs::create_dir_all(&project_b);

        let wiki_a = Wiki::open(&project_a.join(".bbarit"), &project_a).unwrap();
        let wiki_b = Wiki::open(&project_b.join(".bbarit"), &project_b).unwrap();
        wiki_a.set("overview", "project A overview").unwrap();

        // A sees its page; B must NOT — this is the cross-project leak that
        // injected one project's overview into another project's review.
        assert!(wiki_a.list().unwrap().iter().any(|(n, _)| n == "overview"));
        assert!(wiki_b.list().unwrap().is_empty());
        assert!(wiki_b.search("overview").unwrap().is_empty());
        assert_eq!(wiki_b.get("overview").unwrap(), None);

        // Top-level vault notes (other projects' legacy pages, the user's own
        // notes) are COMPLETELY invisible to the agent — even by exact name.
        // Guessable names like "overview" were the observed leak vector.
        std::fs::write(vault.join("shared-note.md"), "hand-written").unwrap();
        assert!(
            !wiki_b
                .list()
                .unwrap()
                .iter()
                .any(|(n, _)| n == "shared-note")
        );
        assert_eq!(wiki_b.get("shared-note").unwrap(), None);
        assert!(!wiki_b.delete("shared-note").unwrap());
        assert!(vault.join("shared-note.md").exists(), "user note untouched");

        unsafe { std::env::remove_var("BBARIT_NOTE_VAULT_DIR") };
    }
}
