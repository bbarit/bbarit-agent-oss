//! Safety net for file mutations: before the agent overwrites or edits a file,
//! the previous content is snapshotted under the session directory. `/restore`
//! lists those snapshots and can put any of them back — so a bad edit or an
//! overzealous rewrite is always reversible.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub seq: u64,
    /// Original absolute path of the file that was about to change.
    pub path: String,
    /// Snapshot file name inside the checkpoint directory.
    pub snapshot: String,
    pub at: String,
    /// Which tool was about to change it (write/edit/append).
    pub tool: String,
}

/// Don't snapshot huge files — the agent shouldn't be blind-editing those
/// anyway, and the checkpoint dir must stay cheap.
const MAX_SNAPSHOT_BYTES: u64 = 2 * 1024 * 1024;
/// Per-session cap so a long session can't grow the directory unboundedly.
const MAX_CHECKPOINTS: usize = 200;

fn checkpoint_dir(session_dir: &Path, session_id: &str) -> PathBuf {
    session_dir.join("checkpoints").join(session_id)
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.jsonl")
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Read the checkpoint index (empty when none exist yet).
pub fn list(session_dir: &Path, session_id: &str) -> Vec<Checkpoint> {
    let dir = checkpoint_dir(session_dir, session_id);
    let Ok(text) = fs::read_to_string(index_path(&dir)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Checkpoint>(line).ok())
        .collect()
}

/// Snapshot `target` before a mutation. Best-effort by design: a failed
/// snapshot must never block the actual work, so errors are swallowed.
pub fn record(session_dir: &Path, session_id: &str, tool: &str, target: &Path) {
    let _ = record_inner(session_dir, session_id, tool, target);
}

fn record_inner(session_dir: &Path, session_id: &str, tool: &str, target: &Path) -> Result<()> {
    let Ok(meta) = fs::metadata(target) else {
        return Ok(()); // brand-new file — nothing to preserve
    };
    if !meta.is_file() || meta.len() > MAX_SNAPSHOT_BYTES {
        return Ok(());
    }
    let bytes = fs::read(target)?;
    let dir = checkpoint_dir(session_dir, session_id);
    fs::create_dir_all(&dir)?;

    let existing = list(session_dir, session_id);
    if existing.len() >= MAX_CHECKPOINTS {
        return Ok(());
    }
    // Skip when the newest snapshot of this same file already has this content
    // (e.g. several edits in one turn each triggering a snapshot).
    if let Some(last) = existing
        .iter()
        .rev()
        .find(|c| c.path == target.display().to_string())
        && fs::read(dir.join(&last.snapshot))
            .map(|prev| prev == bytes)
            .unwrap_or(false)
    {
        return Ok(());
    }

    let seq = existing.last().map(|c| c.seq + 1).unwrap_or(1);
    let file_name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let snapshot = format!("{seq:04}_{}", sanitize_name(&file_name));
    fs::write(dir.join(&snapshot), &bytes)?;

    let entry = Checkpoint {
        seq,
        path: target.display().to_string(),
        snapshot,
        at: chrono::Utc::now().to_rfc3339(),
        tool: tool.to_string(),
    };
    let mut line = serde_json::to_string(&entry)?;
    line.push('\n');
    use std::io::Write;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(index_path(&dir))?
        .write_all(line.as_bytes())?;
    Ok(())
}

/// Restore one checkpoint by sequence number. Returns a human summary.
pub fn restore(session_dir: &Path, session_id: &str, seq: u64) -> Result<String> {
    let dir = checkpoint_dir(session_dir, session_id);
    let entry = list(session_dir, session_id)
        .into_iter()
        .find(|c| c.seq == seq)
        .ok_or_else(|| anyhow!("no checkpoint #{seq} — run /restore to list them"))?;
    let bytes = fs::read(dir.join(&entry.snapshot))
        .with_context(|| format!("checkpoint data missing for #{seq}"))?;
    if let Some(parent) = Path::new(&entry.path).parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&entry.path, bytes).with_context(|| format!("write {}", entry.path))?;
    Ok(format!(
        "Restored {} from checkpoint #{seq} ({}).",
        entry.path, entry.at
    ))
}

/// Restore every touched file to its EARLIEST snapshot — i.e. undo all of this
/// session's mutations to files that existed before it.
pub fn restore_all(session_dir: &Path, session_id: &str) -> Result<String> {
    let entries = list(session_dir, session_id);
    if entries.is_empty() {
        return Ok("No checkpoints in this session.".to_string());
    }
    let mut earliest: std::collections::HashMap<String, &Checkpoint> =
        std::collections::HashMap::new();
    for entry in &entries {
        earliest.entry(entry.path.clone()).or_insert(entry);
    }
    let mut restored = Vec::new();
    for (path, entry) in earliest {
        let dir = checkpoint_dir(session_dir, session_id);
        if let Ok(bytes) = fs::read(dir.join(&entry.snapshot))
            && fs::write(&path, bytes).is_ok()
        {
            restored.push(format!("  - {path} (from #{})", entry.seq));
        }
    }
    restored.sort();
    Ok(format!(
        "Restored {} file(s) to their pre-session content:\n{}\n(Note: files newly CREATED this session are left in place.)",
        restored.len(),
        restored.join("\n")
    ))
}

/// Render the checkpoint list for `/restore` with no arguments.
pub fn render_list(session_dir: &Path, session_id: &str) -> String {
    let entries = list(session_dir, session_id);
    if entries.is_empty() {
        return "No checkpoints yet — one is saved automatically before every file change. \
                Usage: /restore <n> to roll one back, /restore all to undo every change."
            .to_string();
    }
    let mut out = vec![format!("Checkpoints ({} — newest last):", entries.len())];
    for entry in &entries {
        out.push(format!(
            "  #{:<3} {}  [{} @ {}]",
            entry.seq, entry.path, entry.tool, entry.at
        ));
    }
    out.push(
        "Usage: /restore <n> to roll one file back, /restore all to undo every change.".to_string(),
    );
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("bbarit-ckpt-{name}"));
        let _ = fs::remove_dir_all(&base);
        let sessions = base.join("sessions");
        let work = base.join("work");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(&work).unwrap();
        (sessions, work)
    }

    #[test]
    fn snapshot_and_restore_roundtrip() {
        let (sessions, work) = setup("roundtrip");
        let file = work.join("main.rs");
        fs::write(&file, "original").unwrap();
        record(&sessions, "s1", "edit", &file);
        fs::write(&file, "mutated").unwrap();
        let entries = list(&sessions, "s1");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tool, "edit");
        let message = restore(&sessions, "s1", entries[0].seq).unwrap();
        assert!(message.contains("Restored"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn missing_file_and_duplicates_are_skipped() {
        let (sessions, work) = setup("skips");
        // None: nonexistent target records nothing.
        record(&sessions, "s1", "write", &work.join("ghost.txt"));
        assert!(list(&sessions, "s1").is_empty());
        // Mapping: identical consecutive content dedupes to one snapshot.
        let file = work.join("a.txt");
        fs::write(&file, "same").unwrap();
        record(&sessions, "s1", "edit", &file);
        record(&sessions, "s1", "edit", &file);
        assert_eq!(list(&sessions, "s1").len(), 1);
        // Changed content records a second snapshot.
        fs::write(&file, "changed").unwrap();
        record(&sessions, "s1", "edit", &file);
        assert_eq!(list(&sessions, "s1").len(), 2);
    }

    #[test]
    fn restore_all_uses_earliest_snapshot() {
        let (sessions, work) = setup("all");
        let file = work.join("app.py");
        fs::write(&file, "v1").unwrap();
        record(&sessions, "s1", "edit", &file);
        fs::write(&file, "v2").unwrap();
        record(&sessions, "s1", "edit", &file);
        fs::write(&file, "v3").unwrap();
        let message = restore_all(&sessions, "s1").unwrap();
        assert!(message.contains("1 file"), "{message}");
        assert_eq!(fs::read_to_string(&file).unwrap(), "v1");
    }

    #[test]
    fn bad_seq_is_a_clear_error() {
        let (sessions, _) = setup("bad-seq");
        let error = restore(&sessions, "s1", 99).unwrap_err().to_string();
        assert!(error.contains("no checkpoint #99"), "{error}");
    }
}
