//! Interactive TUI built on ratatui: a Claude Code / Codex style single-column
//! chat — a scrolling, word-wrapped transcript with an input box and a footer
//! menu. The agent turn runs on a worker thread so the UI keeps rendering
//! (spinner-free live streaming) and never freezes during long model calls.

use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::commands::handle_input;
use crate::config::AppConfig;
use crate::providers::Registry;
use crate::session::{INPUT_HISTORY_FILE, Role, SessionStore};

/// While the TUI (raw mode + alternate screen) is up, redirect stderr (fd 2) to a log file.
/// An `eprintln!` (tool-arg parse failure, persona load error, etc.) printed straight to the PTY
/// would "type" the error text at the cursor — usually the input box — and, since ratatui only
/// updates cell diffs, the polluted line lingers and hides the bottom UI (working bar / input box).
/// Drop restores the original fd, so normal exit, early return, and panic unwinding are all safe.
struct StderrRedirectGuard {
    #[cfg(unix)]
    saved_fd: i32,
}

/// The panic hook runs before Drop, so we share the original fd so stderr can be restored
/// from inside the hook too (-1 = no redirect).
#[cfg(unix)]
static SAVED_STDERR_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Call on hook entry so panic messages aren't hidden in the log file — idempotent.
fn restore_stderr_for_panic() {
    #[cfg(unix)]
    {
        let fd = SAVED_STDERR_FD.swap(-1, std::sync::atomic::Ordering::SeqCst);
        if fd >= 0 {
            unsafe {
                libc::dup2(fd, 2);
                // Drop closes the fd itself (avoids a double close) — here we only restore.
            }
        }
    }
}

impl StderrRedirectGuard {
    fn install() -> Option<Self> {
        #[cfg(unix)]
        {
            let dir = dirs_next::home_dir()?
                .join(crate::config::USER_APP_ROOT)
                .join("agent");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("tui-stderr.log");
            // Prevent unbounded growth: start fresh past 1MB.
            if std::fs::metadata(&path)
                .map(|m| m.len() > 1_000_000)
                .unwrap_or(false)
            {
                let _ = std::fs::remove_file(&path);
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()?;
            use std::os::unix::io::IntoRawFd;
            let file_fd = file.into_raw_fd();
            unsafe {
                let saved_fd = libc::dup(2);
                if saved_fd < 0 {
                    libc::close(file_fd);
                    return None;
                }
                libc::dup2(file_fd, 2);
                libc::close(file_fd);
                SAVED_STDERR_FD.store(saved_fd, std::sync::atomic::Ordering::SeqCst);
                Some(Self { saved_fd })
            }
        }
        #[cfg(not(unix))]
        {
            // Windows: swapping the console handle is too risky, so it's deferred. eprintln pollution
            // has only been reported on a Unix PTY (embedded terminal).
            None
        }
    }
}

impl Drop for StderrRedirectGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // If the panic hook already restored (swapped to -1), skip dup2 and just close the fd.
            let not_yet_restored = SAVED_STDERR_FD
                .compare_exchange(
                    self.saved_fd,
                    -1,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok();
            unsafe {
                if not_yet_restored {
                    libc::dup2(self.saved_fd, 2);
                }
                libc::close(self.saved_fd);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    User,
    Assistant,
    Tool,
    System,
    /// A startup splash (the BBARIT logo): rendered as colored art with no
    /// role prefix.
    Banner,
}

struct Entry {
    kind: Kind,
    text: String,
    /// Stable identity for the per-entry render cache: entries are immutable
    /// once pushed, so (id, width, tools_expanded) fully keys their rendered
    /// lines — no per-frame hashing of the full text.
    id: u64,
}

impl Entry {
    fn new(kind: Kind, text: String) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            kind,
            text,
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// A filterable picker overlay (models, sessions, or the command menu).
struct Selector {
    title: String,
    /// (display label, command to run on select).
    items: Vec<(String, String)>,
    filter: String,
    cursor: usize,
    /// Optional rich rendering per item index (colored usage gauges in the
    /// accounts dashboard). Items without an entry render their plain label.
    /// The plain label still drives filtering.
    styled: std::collections::HashMap<usize, Line<'static>>,
}

impl Selector {
    fn filtered_indexed(&self) -> Vec<(usize, &(String, String))> {
        let needle = self.filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, (label, command))| {
                needle.is_empty()
                    || label.to_lowercase().contains(&needle)
                    || command.to_lowercase().contains(&needle)
            })
            .collect()
    }

    fn filtered(&self) -> Vec<&(String, String)> {
        self.filtered_indexed()
            .into_iter()
            .map(|(_, item)| item)
            .collect()
    }

    fn selected_command(&self) -> Option<String> {
        self.filtered()
            .get(self.cursor)
            .map(|(_, command)| command.clone())
            .filter(|command| !command.is_empty())
    }

    /// First selectable (non-header) row in the filtered view.
    fn first_selectable(&self) -> usize {
        self.filtered()
            .iter()
            .position(|(_, command)| !command.is_empty())
            .unwrap_or(0)
    }

    /// Move the cursor by `delta` selectable rows, skipping header rows (which
    /// have an empty command).
    fn move_cursor(&mut self, delta: i32) {
        let selectable: Vec<usize> = self
            .filtered()
            .iter()
            .enumerate()
            .filter(|(_, (_, command))| !command.is_empty())
            .map(|(index, _)| index)
            .collect();
        if selectable.is_empty() {
            return;
        }
        let position = selectable.iter().position(|&index| index == self.cursor);
        self.cursor = match position {
            Some(current) => {
                let target = current as i32 + delta;
                if target < 0 || target >= selectable.len() as i32 {
                    return;
                }
                selectable[target as usize]
            }
            None => selectable[0],
        };
    }
}

/// Build a link list grouped by domain (brand): a header row per domain with
/// its URLs underneath. Header rows have an empty command and are not
/// selectable; URL rows carry the URL as their command.
fn links_selector(entries: &[Entry]) -> Selector {
    let pattern = regex::Regex::new(r#"https?://[^\s<>"')\]]+"#).expect("valid url regex");
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        for found in pattern.find_iter(&entry.text) {
            let url = found
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '"', '\''])
                .to_string();
            if url.is_empty() || !seen.insert(url.clone()) {
                continue;
            }
            let brand = brand_name(&url_brand(&url));
            match groups.iter_mut().find(|(name, _)| *name == brand) {
                Some(group) => group.1.push(url),
                None => groups.push((brand, vec![url])),
            }
        }
    }
    let mut items: Vec<(String, String)> = vec![("Back to menu".to_string(), "@menu".to_string())];
    if groups.is_empty() {
        items.push(("(no links in this conversation)".to_string(), String::new()));
    }
    for (brand, urls) in &groups {
        items.push((format!("[{brand}]"), String::new()));
        for url in urls {
            items.push((format!("   {url}"), url.clone()));
        }
    }
    let mut selector = Selector {
        title: "Links in conversation".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    };
    selector.cursor = selector.first_selectable();
    selector
}

/// Brand/domain for grouping: host without scheme, userinfo, port, or www.
fn url_brand(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url);
    let host = host.split('/').next().unwrap_or(host);
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// Map a host to a friendly brand name for grouping headers, so related
/// domains (github.com, raw.githubusercontent.com) land under one group.
/// Falls back to the host itself for unknown domains.
fn brand_name(host: &str) -> String {
    const MAP: &[(&str, &str)] = &[
        ("githubusercontent.com", "GitHub"),
        ("github.io", "GitHub"),
        ("github.com", "GitHub"),
        ("gitlab.com", "GitLab"),
        ("bitbucket.org", "Bitbucket"),
        ("openai.com", "OpenAI"),
        ("anthropic.com", "Anthropic"),
        ("claude.ai", "Anthropic"),
        ("googleapis.com", "Google"),
        ("google.com", "Google"),
        ("youtube.com", "YouTube"),
        ("youtu.be", "YouTube"),
        ("stackoverflow.com", "Stack Overflow"),
        ("rust-lang.org", "Rust"),
        ("crates.io", "Rust"),
        ("docs.rs", "Rust"),
        ("ratatui.rs", "Ratatui"),
        ("huggingface.co", "Hugging Face"),
        ("npmjs.com", "npm"),
        ("microsoft.com", "Microsoft"),
        ("azure.com", "Azure"),
        ("cloudflare.com", "Cloudflare"),
        ("openrouter.ai", "OpenRouter"),
        ("deepseek.com", "DeepSeek"),
        ("mistral.ai", "Mistral"),
        ("x.ai", "xAI"),
        ("wikipedia.org", "Wikipedia"),
        ("medium.com", "Medium"),
        ("reddit.com", "Reddit"),
    ];
    let lower = host.to_lowercase();
    for (suffix, name) in MAP {
        if lower == *suffix || lower.ends_with(&format!(".{suffix}")) {
            return (*name).to_string();
        }
    }
    if lower == "localhost" {
        return "Local".to_string();
    }
    host.to_string()
}

/// Consolidate related provider ids under one family name (Bedrock, Cloudflare,
/// Azure, Vertex, Vercel, Ollama); other providers use their registry name.
fn provider_group_name(registry: &Registry, provider_id: &str) -> String {
    let lower = provider_id.to_lowercase();
    if lower.contains("bedrock") {
        "Amazon Bedrock".to_string()
    } else if lower.contains("cloudflare") {
        "Cloudflare".to_string()
    } else if lower.starts_with("azure") {
        "Azure OpenAI".to_string()
    } else if lower.contains("vertex") {
        "Google Vertex".to_string()
    } else if lower.contains("vercel") {
        "Vercel AI Gateway".to_string()
    } else if lower == "ollama" {
        "Ollama (local)".to_string()
    } else {
        registry
            .provider(provider_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| provider_id.to_string())
    }
}

/// Folder picker for switching the working codebase mid-session: recent
/// projects, the parent directory, and immediate subdirectories of the cwd.
/// Each item's command is `@cd:<path>`.
fn folder_selector(config: &AppConfig) -> Selector {
    let cwd = config.cwd.clone();
    let mut items: Vec<(String, String)> = vec![("Back to menu".to_string(), "@menu".to_string())];
    let mut seen = std::collections::HashSet::new();
    seen.insert(cwd.clone());

    items.push(("Recent / nearby folders".to_string(), String::new()));
    for path in crate::project::recent_projects() {
        if seen.insert(path.clone()) {
            items.push((
                format!("  {}", path.display()),
                format!("@cd:{}", path.display()),
            ));
        }
    }
    if let Some(parent) = cwd.parent()
        && seen.insert(parent.to_path_buf())
    {
        items.push((
            format!("  .. {}", parent.display()),
            format!("@cd:{}", parent.display()),
        ));
    }
    if let Ok(entries) = std::fs::read_dir(&cwd) {
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && !p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .starts_with('.')
            })
            .collect();
        subdirs.sort();
        for path in subdirs {
            if seen.insert(path.clone()) {
                items.push((
                    format!("  {}", path.display()),
                    format!("@cd:{}", path.display()),
                ));
            }
        }
    }
    if items.len() == 2 {
        items.push((
            "  (type /cd <path> to switch to any folder)".to_string(),
            String::new(),
        ));
    }
    Selector {
        title: "Change codebase folder".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    }
}

/// Switch the working directory mid-session: tools, the system-prompt cwd, and
/// the footer all follow `config.cwd`, so updating it retargets the agent.
fn change_codebase(
    app: &mut App,
    store: &SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    raw_path: &str,
) {
    let target = expand_home(raw_path.trim());
    let resolved = if target.is_absolute() {
        target
    } else {
        config.cwd.join(target)
    };
    let canonical = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    if !canonical.is_dir() {
        app.entries.push(Entry::new(
            Kind::System,
            format!("Not a folder: {}", canonical.display()),
        ));
        return;
    }
    if std::env::set_current_dir(&canonical).is_err() {
        app.entries.push(Entry::new(
            Kind::System,
            format!("Could not enter {}", canonical.display()),
        ));
        return;
    }
    config.cwd = canonical.clone();
    // /cd out of the session worktree invalidates the worktree bookkeeping —
    // a later /worktree merge must not squash a stale branch from elsewhere.
    {
        let mut origin = WORKTREE_ORIGIN.lock().unwrap();
        let left = origin.as_ref().is_some_and(|(_, wt)| {
            let wt = std::fs::canonicalize(wt).unwrap_or_else(|_| wt.clone());
            !canonical.starts_with(&wt)
        });
        if left {
            *origin = None;
            app.entries.push(Entry::new(
                Kind::System,
                "Left the session worktree — worktree tracking cleared.".to_string(),
            ));
        }
    }
    crate::project::remember_project(&canonical);
    app.entries.push(Entry::new(
        Kind::System,
        format!(
            "Codebase folder is now {} — new files and commands run here.",
            canonical.display()
        ),
    ));
    app.status = status_line(store, registry, config);
    app.title = title_line(store, config);
    refresh_tabs(app, store, config);
    app.follow = true;
}

/// `~`/`~/...` come in literally from /cd; joining them onto cwd would target
/// a directory literally named "~".
fn expand_home(raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs_next::home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\"))
        && let Some(home) = dirs_next::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

/// Worktree bookkeeping for this session: (the cwd we entered the worktree
/// from, the worktree path itself). `/worktree off` returns to the origin; the
/// recorded path pins merge/off to the worktree we actually created, so after
/// a /cd into some other repo that repo can't get squash-merged by mistake.
/// None when not in a worktree we created this session.
static WORKTREE_ORIGIN: std::sync::Mutex<Option<(PathBuf, PathBuf)>> = std::sync::Mutex::new(None);

/// Record (origin, worktree) for merge/off. Nested /worktree calls keep the
/// first, outermost origin but always track the latest worktree.
fn remember_worktree(config: &AppConfig, wt_path: &std::path::Path) {
    let mut origin = WORKTREE_ORIGIN.lock().unwrap();
    let base = origin
        .take()
        .map(|(base, _)| base)
        .unwrap_or_else(|| config.cwd.clone());
    *origin = Some((base, wt_path.to_path_buf()));
}

fn note(app: &mut App, text: String) {
    app.entries.push(Entry::new(Kind::System, text));
}

/// `/worktree <branch>` — create (or reset) a git worktree on `branch` under
/// `.bbarit/worktrees/` and switch the agent into it, so risky work stays
/// isolated on its own branch. `/worktree off` returns to where you started.
fn enter_worktree(
    app: &mut App,
    store: &SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    branch: &str,
) {
    let branch = branch.trim();
    if branch.is_empty() {
        note(
            app,
            "usage: /worktree <branch>  (or /worktree off to leave)".to_string(),
        );
        return;
    }
    let repo_root = match git_main_root(&config.cwd) {
        Some(root) => root,
        None => {
            note(
                app,
                "Not inside a git repository — can't create a worktree.".to_string(),
            );
            return;
        }
    };
    let dir_name = match worktree_dir_name(branch) {
        Some(name) => name,
        None => {
            note(app, format!("'{branch}' is not usable as a worktree name."));
            return;
        }
    };
    let wt_path = repo_root
        .join(crate::config::APP_DIR)
        .join("worktrees")
        .join(dir_name);
    let _ = std::fs::create_dir_all(wt_path.parent().unwrap_or(&repo_root));
    // An existing branch is checked out as-is: -B would silently reset it to
    // the current HEAD, orphaning its commits.
    let branch_exists = run_git_in(
        &repo_root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok();
    let mut command = crate::spawn::no_window_command("git");
    command.arg("-C").arg(&repo_root).arg("worktree").arg("add");
    if branch_exists {
        command.arg(&wt_path).arg(branch);
    } else {
        command.arg("-B").arg(branch).arg(&wt_path);
    }
    match command.output() {
        Ok(out) if out.status.success() => {
            remember_worktree(config, &wt_path);
            exclude_worktrees_dir(&repo_root);
            note(
                app,
                format!(
                    "Entered worktree on branch '{branch}'. Work here is isolated; /worktree off to return."
                ),
            );
            change_codebase(app, store, registry, config, &wt_path.to_string_lossy());
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            // A stale worktree dir may already exist; reuse it only when it
            // really has the requested branch checked out.
            let on_branch = wt_path.is_dir()
                && run_git_in(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"])
                    .is_ok_and(|head| head == branch);
            if on_branch {
                remember_worktree(config, &wt_path);
                exclude_worktrees_dir(&repo_root);
                note(app, format!("Reusing existing worktree on '{branch}'."));
                change_codebase(app, store, registry, config, &wt_path.to_string_lossy());
            } else {
                note(app, format!("git worktree add failed: {}", err.trim()));
            }
        }
        Err(error) => note(app, format!("Could not run git: {error}")),
    }
}

/// Directory name for a worktree branch: unsafe chars become '-', '/' merges
/// into '-'. None when the result is empty or a path traversal (".", ".."),
/// e.g. `/worktree ..` must not resolve to `.bbarit` itself.
fn worktree_dir_name(branch: &str) -> Option<String> {
    let safe: String = branch
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = safe.replace('/', "-");
    if name.is_empty() || name == "." || name == ".." {
        None
    } else {
        Some(name)
    }
}

/// Best-effort: keep `.bbarit/worktrees/` out of `git status` in the origin
/// repo (`.git/info/exclude` is local, so the tracked .gitignore stays clean).
fn exclude_worktrees_dir(repo_root: &std::path::Path) {
    let Ok(git_dir) = run_git_in(repo_root, &["rev-parse", "--git-common-dir"]) else {
        return;
    };
    let git_dir = PathBuf::from(git_dir);
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        repo_root.join(git_dir)
    };
    let exclude = git_dir.join("info").join("exclude");
    let mut existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim() == ".bbarit-oss/worktrees/")
    {
        return;
    }
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(".bbarit-oss/worktrees/\n");
    let _ = std::fs::create_dir_all(exclude.parent().unwrap_or(&git_dir));
    let _ = std::fs::write(&exclude, existing);
}

/// Run git in `dir`, returning trimmed stdout on success and stderr on failure.
fn run_git_in(dir: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = crate::spawn::no_window_command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn git_is_dirty(dir: &std::path::Path) -> bool {
    run_git_in(dir, &["status", "--porcelain"])
        .map(|out| !out.is_empty())
        .unwrap_or(false)
}

/// `/worktree merge` — squash-merge the worktree branch back into the branch
/// the main checkout is on, with stepwise rollback, then remove the worktree
/// and return. The worktree's uncommitted changes are auto-committed first so
/// nothing silently vanishes.
fn merge_worktree(
    app: &mut App,
    store: &SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
) {
    let Some((origin, wt_path)) = WORKTREE_ORIGIN.lock().unwrap().clone() else {
        note(
            app,
            "Not in a session worktree. Use /worktree <branch> first; then /worktree merge \
             folds the branch back."
                .to_string(),
        );
        return;
    };
    // Only the RECORDED worktree merges — never "wherever we are now", or a
    // /cd to another repo would get that repo auto-committed and the original
    // repo squash-merged with a stale branch.
    let here = std::fs::canonicalize(&config.cwd).unwrap_or_else(|_| config.cwd.clone());
    let wt_canonical = std::fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
    if !here.starts_with(&wt_canonical) {
        note(
            app,
            format!(
                "Not inside the session worktree ({}) — /cd back there before /worktree merge.",
                wt_path.display()
            ),
        );
        return;
    }
    let wt_branch = match run_git_in(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) if !branch.is_empty() && branch != "HEAD" => branch,
        Ok(_) => {
            note(
                app,
                "Worktree is on a detached HEAD — nothing to merge.".to_string(),
            );
            return;
        }
        Err(error) => {
            note(app, format!("Could not read the worktree branch: {error}"));
            return;
        }
    };
    let target = match run_git_in(&origin, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) if !branch.is_empty() => branch,
        _ => {
            note(
                app,
                "Could not read the main checkout's branch.".to_string(),
            );
            return;
        }
    };
    if target == wt_branch {
        note(
            app,
            format!("Main checkout is already on '{wt_branch}' — nothing to merge into."),
        );
        return;
    }

    // 1) Nothing in the worktree may be lost: auto-commit its dirty state.
    if git_is_dirty(&wt_path) {
        let _ = run_git_in(&wt_path, &["add", "-A"]);
        if let Err(error) = run_git_in(
            &wt_path,
            &["commit", "-m", "auto-commit: worktree changes before merge"],
        ) {
            note(
                app,
                format!("Could not auto-commit worktree changes: {error}"),
            );
            return;
        }
    }

    // 2) A dirty main checkout is stashed (tracked + untracked) and restored after.
    let stashed = if git_is_dirty(&origin) {
        match run_git_in(
            &origin,
            &["stash", "push", "-u", "-m", "bbarit-worktree-merge"],
        ) {
            Ok(_) => true,
            Err(error) => {
                note(app, format!("Could not stash the main checkout: {error}"));
                return;
            }
        }
    } else {
        false
    };
    let restore_stash = |app: &mut App| {
        if stashed && let Err(error) = run_git_in(&origin, &["stash", "pop"]) {
            note(
                app,
                format!(
                    "⚠ stash pop hit a conflict — your changes are SAFE in the stash \
                     (git stash list). Resolve manually. ({error})"
                ),
            );
        }
    };

    // 3) Squash-merge; on failure roll the main checkout back clean.
    if let Err(error) = run_git_in(&origin, &["merge", "--squash", &wt_branch]) {
        let _ = run_git_in(&origin, &["merge", "--abort"]);
        let _ = run_git_in(&origin, &["reset", "--merge"]);
        restore_stash(app);
        note(
            app,
            format!(
                "Squash merge of '{wt_branch}' into '{target}' failed (conflicts?): {error}\n\
                 The main checkout was rolled back. Resolve in the worktree and retry, or \
                 merge manually."
            ),
        );
        return;
    }
    // Identical trees stage nothing — detect that directly instead of guessing
    // from a failed commit, which could also be hooks or missing user.email
    // and must not be mistaken for "no changes".
    if run_git_in(&origin, &["diff", "--cached", "--quiet"]).is_ok() {
        let _ = run_git_in(&origin, &["reset", "--merge"]);
        restore_stash(app);
        note(
            app,
            format!("'{wt_branch}' has no changes against '{target}' — nothing to merge."),
        );
        return;
    }
    if let Err(error) = run_git_in(
        &origin,
        &["commit", "-m", &format!("squash merge {wt_branch}")],
    ) {
        let _ = run_git_in(&origin, &["reset", "--merge"]);
        restore_stash(app);
        note(
            app,
            format!(
                "git commit failed after the squash merge: {error}\n\
                 The main checkout was rolled back; the worktree and branch '{wt_branch}' are \
                 untouched — fix the cause (hooks, user.email, …) and retry /worktree merge."
            ),
        );
        return;
    }
    restore_stash(app);

    // 4) Return to the main checkout FIRST (the process must not sit inside a
    //    directory it is about to remove), then clean up the worktree+branch.
    WORKTREE_ORIGIN.lock().unwrap().take();
    change_codebase(app, store, registry, config, &origin.to_string_lossy());
    let _ = run_git_in(
        &origin,
        &["worktree", "remove", "--force", &wt_path.to_string_lossy()],
    );
    let _ = run_git_in(&origin, &["branch", "-D", &wt_branch]);
    let sha = run_git_in(&origin, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    note(
        app,
        format!(
            "Squash-merged '{wt_branch}' into '{target}' ({sha}) — worktree removed, branch \
             deleted, back in the main checkout."
        ),
    );
}

fn exit_worktree(app: &mut App, store: &SessionStore, registry: &Registry, config: &mut AppConfig) {
    let recorded = WORKTREE_ORIGIN.lock().unwrap().clone();
    match recorded {
        Some((path, wt_path)) => {
            let here = std::fs::canonicalize(&config.cwd).unwrap_or_else(|_| config.cwd.clone());
            let wt = std::fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
            if !here.starts_with(&wt) {
                note(
                    app,
                    format!(
                        "Not inside the session worktree ({}) — /cd back there before /worktree off.",
                        wt_path.display()
                    ),
                );
                return;
            }
            WORKTREE_ORIGIN.lock().unwrap().take();
            note(
                app,
                "Left the worktree — back to the main checkout.".to_string(),
            );
            change_codebase(app, store, registry, config, &path.to_string_lossy());
        }
        None => note(
            app,
            "Not in a worktree this session. Use /worktree <branch> first.".to_string(),
        ),
    }
}

/// The MAIN repository root from `cwd`: from inside a linked worktree this is
/// still the primary checkout, so new worktrees never nest inside old ones.
fn git_main_root(cwd: &std::path::Path) -> Option<PathBuf> {
    // --git-common-dir points at the main .git even from a linked worktree;
    // it may be relative (".git"), so resolve against cwd.
    let common = run_git_in(cwd, &["rev-parse", "--git-common-dir"]).ok()?;
    if common.is_empty() {
        return None;
    }
    let common = PathBuf::from(common);
    let git_dir = if common.is_absolute() {
        common
    } else {
        cwd.join(common)
    };
    let git_dir = std::fs::canonicalize(&git_dir).ok()?;
    Some(git_dir.parent()?.to_path_buf())
}

/// Step 1 of model selection: pick a provider. Providers you have credentials
/// for are marked ✓ and listed first, so you don't pick a no-key one. Selecting
/// one opens the model picker for that provider (`@prov:<id>` command).
fn provider_selector(registry: &Registry, config: &AppConfig) -> Selector {
    let mut providers: Vec<(String, String, usize, bool)> = registry
        .providers()
        .map(|provider| {
            (
                provider.id.clone(),
                provider.name.clone(),
                registry.models_for_provider(&provider.id).len(),
                crate::commands::provider_has_credentials(registry, config, &provider.id),
            )
        })
        .filter(|(_, _, count, _)| *count > 0)
        .collect();
    // Credentialed providers first, Ollama first within those, then by name.
    providers.sort_by_key(|(id, name, _, has)| (!has, id != "ollama", name.to_lowercase()));
    let mut items = vec![("Back to menu".to_string(), "@menu".to_string())];
    items.extend(providers.into_iter().map(|(id, name, count, has)| {
        let mark = if has { "[key]" } else { "[ ]" };
        (format!("{mark} {name}  ({count})"), format!("@prov:{id}"))
    }));
    Selector {
        title: "Select provider".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    }
}

fn glm_sort_key(value: &str) -> Option<(Vec<u32>, i32)> {
    let lower = value.to_ascii_lowercase();
    let start = lower.find("glm-")? + 4;
    let rest = &lower[start..];
    let mut end = 0;
    for (idx, ch) in rest.char_indices() {
        if ch.is_ascii_digit() || ch == '.' {
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    let numbers: Vec<u32> = rest[..end]
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    if numbers.is_empty() {
        return None;
    }
    let suffix = &rest[end..];
    let variant_rank = if suffix.starts_with("-fast") {
        6
    } else if suffix.is_empty() {
        5
    } else if suffix.contains("turbo") {
        4
    } else if suffix.contains("flash") {
        3
    } else if suffix.contains("-x") {
        2
    } else if suffix.contains("airx") {
        1
    } else if suffix.contains("air") {
        0
    } else if suffix.contains('v') {
        -1
    } else {
        0
    };
    Some((numbers, variant_rank))
}

/// A provider is "ready" (logged in) when it has a stored API key, or it's a
/// local runtime (Ollama) that needs none.
fn provider_ready(config: &AppConfig, provider: &str) -> bool {
    if provider == "ollama" {
        return true;
    }
    matches!(crate::auth::stored_api_key(config, provider), Ok(Some(_)))
}

/// Build a model picker with logged-in providers floated to the top and a pinned
/// "Recent / current" group (current model + favorites) above everything, so the
/// models you can actually use are right there instead of buried in a 1000-row
/// alphabetical list. `command_for(provider, id)` builds the row's command.
fn model_picker_items(
    registry: &Registry,
    config: &AppConfig,
    models: Vec<&crate::providers::Model>,
    command_for: &dyn Fn(&str, &str) -> String,
    leading: Vec<(String, String)>,
    title: String,
    filter: String,
) -> Selector {
    use std::collections::{BTreeMap, HashSet};
    let ready: HashSet<String> = registry
        .providers()
        .filter(|provider| provider_ready(config, &provider.id))
        .map(|provider| provider.id.clone())
        .collect();

    // Recent = the current model then favorites, resolved to provider/id.
    let mut recent_refs: Vec<String> = Vec::new();
    let mut seen_recent: HashSet<String> = HashSet::new();
    let mut add_recent = |reference: &str| {
        if let Some(resolved) = registry.resolve_reference_with_thinking(reference) {
            let key = format!("{}/{}", resolved.model.provider, resolved.model.id);
            if seen_recent.insert(key.clone()) {
                recent_refs.push(key);
            }
        }
    };
    if let Some(model) = &config.model {
        add_recent(model);
    }
    for favorite in &config.favorites {
        add_recent(favorite);
    }

    let mut groups: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut ready_families: HashSet<String> = HashSet::new();
    let mut by_ref: BTreeMap<String, (String, String)> = BTreeMap::new();
    for model in &models {
        let family = provider_group_name(registry, &model.provider);
        if ready.contains(&model.provider) {
            ready_families.insert(family.clone());
        }
        // Context window + reasoning at a glance, so picking doesn't need docs.
        let ctx = model
            .context_window
            .filter(|window| *window > 0)
            .map(|window| format!("  {}k", window / 1000))
            .unwrap_or_default();
        let think = if model.reasoning { "  [thinking]" } else { "" };
        let label = format!("{}  -  {}{ctx}{think}", model.id, model.name);
        let command = command_for(&model.provider, &model.id);
        by_ref.insert(
            format!("{}/{}", model.provider, model.id),
            (label.clone(), command.clone()),
        );
        groups.entry(family).or_default().push((label, command));
    }

    // Local first, then logged-in providers, then the rest alphabetically.
    let mut families: Vec<String> = groups.keys().cloned().collect();
    families.sort_by_key(|name| {
        (
            name != "Ollama (local)",
            !ready_families.contains(name),
            name.clone(),
        )
    });

    let mut items: Vec<(String, String)> = leading;
    let recent_items: Vec<(String, String)> = recent_refs
        .iter()
        .filter_map(|reference| {
            by_ref
                .get(reference)
                .map(|(label, command)| (format!("  {label}"), command.clone()))
        })
        .collect();
    if !recent_items.is_empty() {
        items.push(("★ Recent / current".to_string(), String::new()));
        items.extend(recent_items);
    }
    for family in &families {
        let mut entries = groups.remove(family).unwrap_or_default();
        entries.sort_by(|a, b| match (glm_sort_key(&a.1), glm_sort_key(&b.1)) {
            (Some(a_key), Some(b_key)) => b_key
                .cmp(&a_key)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        });
        // A check mark flags families whose provider is logged in.
        let marker = if ready_families.contains(family) {
            "✓ "
        } else {
            ""
        };
        items.push((
            format!("{marker}[{family}] ({})", entries.len()),
            String::new(),
        ));
        for (label, command) in entries {
            items.push((format!("  {label}"), command));
        }
    }
    let mut selector = Selector {
        title,
        items,
        filter,
        cursor: 0,
        styled: Default::default(),
    };
    selector.cursor = selector.first_selectable();
    selector
}

fn model_selector(
    registry: &Registry,
    config: &AppConfig,
    provider: Option<&str>,
    query: &str,
) -> Selector {
    let models = match provider {
        Some(provider) => registry.models_for_provider(provider),
        None => registry.search_models(""),
    };
    let mut leading = Vec::new();
    if provider.is_some() {
        leading.push((
            "Back to providers".to_string(),
            "@back:providers".to_string(),
        ));
    }
    leading.push(("Back to menu".to_string(), "@menu".to_string()));
    let title = match provider {
        Some(provider) => format!("Select model ({provider})"),
        None => "Select model".to_string(),
    };
    model_picker_items(
        registry,
        config,
        models,
        &|provider, id| format!("/model {provider}/{id}"),
        leading,
        title,
        query.to_string(),
    )
}

fn harness_model_selector(
    registry: &Registry,
    config: &AppConfig,
    role: Option<&str>,
    show_all: bool,
) -> Selector {
    let all_models = registry.search_models("");
    // Default to a SHORT list: only models from logged-in providers (the ones
    // you can actually use). Picking a role model out of 1000+ entries was the
    // painful part. "Show all models" expands to the full catalog. Fall back to
    // the full list when nothing is logged in, so it's never empty.
    let ready: std::collections::HashSet<String> = registry
        .providers()
        .filter(|provider| provider_ready(config, &provider.id))
        .map(|provider| provider.id.clone())
        .collect();
    let models: Vec<&crate::providers::Model> = if show_all {
        all_models
    } else {
        let filtered: Vec<&crate::providers::Model> = all_models
            .iter()
            .copied()
            .filter(|model| ready.contains(&model.provider))
            .collect();
        if filtered.is_empty() {
            registry.search_models("")
        } else {
            filtered
        }
    };
    let role_key = role.unwrap_or("all");
    let mut leading = vec![
        (
            "Back to harness roles".to_string(),
            "@back:roles".to_string(),
        ),
        ("Back to menu".to_string(), "@menu".to_string()),
    ];
    if !show_all {
        leading.push((
            "Show all models…".to_string(),
            format!("@roleall:{role_key}"),
        ));
    }
    let title = match role {
        Some(role) => format!("Select {role} model"),
        None => "Select one model for all harness roles".to_string(),
    };
    let role_owned = role.map(str::to_string);
    model_picker_items(
        registry,
        config,
        models,
        &move |provider, id| {
            let model_ref = format!("{provider}/{id}");
            match &role_owned {
                Some(role) => format!("/roles {role} {model_ref}"),
                None => format!("/roles {model_ref}"),
            }
        },
        leading,
        title,
        String::new(),
    )
}

/// One-stop settings dashboard: every session/harness knob with its CURRENT
/// value on the row — pick a row to jump straight into that picker. This is
/// the convenient front door; the individual commands still work.
fn settings_selector(store: &SessionStore, registry: &Registry, config: &AppConfig) -> Selector {
    let model_label = store
        .session()
        .current_model
        .clone()
        .or_else(|| {
            config
                .model
                .as_ref()
                .map(|model| format!("{}/{model}", config.provider))
        })
        .unwrap_or_else(|| "(default)".to_string());
    let thinking = crate::commands::current_thinking_level(store, registry, config);
    let persona_label = crate::personas::effective_persona(config)
        .map(|p| format!("{} {}", p.emoji, p.name))
        .unwrap_or_else(|| "none".to_string());
    let short = |reference: &str| -> String {
        if reference.trim().is_empty() {
            "current model".to_string()
        } else {
            reference
                .rsplit('/')
                .next()
                .unwrap_or(reference)
                .to_string()
        }
    };

    let mut items: Vec<(String, String)> = vec![
        ("Close".to_string(), "@close".to_string()),
        ("Session".to_string(), String::new()),
        (
            format!("  Model      → {model_label}"),
            "/model".to_string(),
        ),
        (
            format!("  Thinking   → {}", thinking.as_str()),
            "/thinking".to_string(),
        ),
        (
            format!("  Persona    → {persona_label}"),
            "@personas".to_string(),
        ),
        (
            "Harness roles (plan → develop → review)".to_string(),
            String::new(),
        ),
    ];
    let role_personas = crate::commands::harness_persona_assignments(config);
    for (role, reference) in crate::commands::harness_role_assignments(config) {
        let nice = match role.as_str() {
            "planner" => "Planner",
            "developer" => "Developer",
            "reviewer" => "Reviewer",
            other => other,
        };
        let persona = role_personas
            .iter()
            .find(|(name, _)| *name == role)
            .map(|(_, id)| id.as_str())
            .filter(|id| !id.is_empty())
            .unwrap_or("no persona");
        items.push((
            format!("  {nice:<9} → {} · {persona}", short(&reference)),
            format!("@role:{role}"),
        ));
    }
    items.extend(
        [
            ("  Role personas (per stage)", "/roles"),
            ("  Same model for all roles", "@role:all"),
            ("  Reset roles to current model", "/roles clear"),
            ("Environment", ""),
        ]
        .into_iter()
        .map(|(label, command)| (label.to_string(), command.to_string())),
    );
    items.push((
        format!("  Codebase   → {}", config.cwd.display()),
        "/cd".to_string(),
    ));
    items.push(("  Login provider".to_string(), "/login".to_string()));
    items.push(("  Accounts & usage".to_string(), "/accounts".to_string()));
    items.push((
        format!(
            "  Claude/Codex interop: {} (reuse their MCP & skills)",
            if crate::mcp::interop_enabled() {
                "ON"
            } else {
                "OFF"
            }
        ),
        "/interop ".to_string(),
    ));
    items.push(("  Add MCP server".to_string(), "/mcp add ".to_string()));
    items.push((
        "  New skill (scaffold)".to_string(),
        "/skill new ".to_string(),
    ));
    let mut selector = Selector {
        title: "Settings — pick a row to change it".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    };
    selector.cursor = selector.first_selectable();
    selector
}

/// Persona picker: every persona grouped by division, plus "off". Selecting
/// one runs `/persona <id>` — same as typing it, but browsable. With
/// `role`, it assigns the persona to that HARNESS role instead
/// (`/roles <role> persona <id>`).
fn persona_selector(config: &AppConfig, role: Option<&str>) -> Selector {
    let personas = crate::personas::load_personas(config);
    let active = match role {
        Some(role) => crate::commands::harness_persona_assignments(config)
            .into_iter()
            .find(|(name, _)| name == role)
            .map(|(_, id)| id)
            .filter(|id| !id.is_empty()),
        None => crate::personas::effective_persona(config).map(|p| p.id),
    };
    let command_for = |id: &str| match role {
        Some(role) => format!("/roles {role} persona {id}"),
        None => format!("/persona {id}"),
    };
    let mut items: Vec<(String, String)> = vec![
        (
            if role.is_some() {
                "Back to harness roles"
            } else {
                "Back to menu"
            }
            .to_string(),
            if role.is_some() {
                "@back:roles"
            } else {
                "@menu"
            }
            .to_string(),
        ),
        // persona_command accepts clear/off/none for both forms.
        (
            "No persona (default behavior)".to_string(),
            command_for("clear"),
        ),
    ];
    let mut division = String::new();
    for p in &personas {
        if p.division != division {
            division = p.division.clone();
            items.push((division.clone(), String::new()));
        }
        let mark = if active.as_deref() == Some(p.id.as_str()) {
            "*"
        } else {
            " "
        };
        items.push((
            format!("{mark} {} {} — {}", p.emoji, p.name, p.description),
            command_for(&p.id),
        ));
    }
    let title = match role {
        Some(role) => format!("Persona for harness {role} ({} available)", personas.len()),
        None => format!("Adopt a persona ({} available)", personas.len()),
    };
    let mut selector = Selector {
        title,
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    };
    selector.cursor = selector.first_selectable();
    selector
}

fn roles_selector(config: &AppConfig) -> Selector {
    // Show what each role is set to right in the menu, so it's clear at a glance
    // and you just pick the role to change (no separate "show" step).
    let assignments = crate::commands::harness_role_assignments(config);
    let short = |reference: &str| -> String {
        if reference.trim().is_empty() {
            "current model".to_string()
        } else {
            // Drop the provider prefix for a compact label.
            reference
                .rsplit('/')
                .next()
                .unwrap_or(reference)
                .to_string()
        }
    };
    let personas = crate::commands::harness_persona_assignments(config);
    let persona_of = |role: &str| -> String {
        personas
            .iter()
            .find(|(name, _)| name == role)
            .map(|(_, id)| id.clone())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| "none".to_string())
    };
    let mut items: Vec<(String, String)> = vec![
        ("Back to menu".to_string(), "@menu".to_string()),
        (
            "Models — pick a role to set its model".to_string(),
            String::new(),
        ),
    ];
    for (role, reference) in &assignments {
        let nice = match role.as_str() {
            "planner" => "Planner",
            "developer" => "Developer",
            "reviewer" => "Reviewer",
            other => other,
        };
        items.push((
            format!("{nice:<10}→ {}", short(reference)),
            format!("@role:{role}"),
        ));
    }
    items.push((
        "Personas — pick a role to set its persona".to_string(),
        String::new(),
    ));
    for (role, _) in &assignments {
        let nice = match role.as_str() {
            "planner" => "Planner",
            "developer" => "Developer",
            "reviewer" => "Reviewer",
            other => other,
        };
        items.push((
            format!("{nice:<10}→ {}", persona_of(role)),
            format!("@rolepersona:{role}"),
        ));
    }
    items.extend(
        [
            ("Quick", ""),
            ("Same model for all roles", "@role:all"),
            ("Use current chat model for all", "/roles current"),
            ("Reset", ""),
            ("Clear custom role models", "/roles clear"),
            ("Preset: GLM", "/roles glm"),
        ]
        .into_iter()
        .map(|(label, command)| (label.to_string(), command.to_string())),
    );
    let mut selector = Selector {
        title: "Harness role models".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    };
    selector.cursor = selector.first_selectable();
    selector
}

/// "MM-DD HH:MM" from an ISO timestamp, for compact session rows.
fn short_when(iso: &str) -> String {
    let date = iso.get(5..10).unwrap_or("");
    let time = iso
        .split_once('T')
        .and_then(|(_, rest)| rest.get(..5))
        .unwrap_or("");
    format!("{date} {time}").trim().to_string()
}

/// Compact "N ago" for the resume list, e.g. "5m ago" / "2h ago" / "3d ago".
fn format_ago(secs: u64) -> String {
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

/// How long since the session file was last written (= last activity).
/// Empty when the file cannot be read, so the label degrades gracefully.
fn session_ago(path: &str) -> String {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .map(|elapsed| format_ago(elapsed.as_secs()))
        .unwrap_or_default()
}

/// Pick the reasoning effort. Each model maps these to its own scale, so a menu
/// is far easier than remembering per-model names. The current level is marked.
fn thinking_selector(current: crate::providers::ThinkingLevel) -> Selector {
    use crate::providers::ThinkingLevel as TL;
    let levels = [
        (TL::Off, "off", "no extra reasoning (fastest)"),
        (TL::Minimal, "minimal", "a little reasoning"),
        (TL::Low, "low", "light reasoning"),
        (TL::Medium, "medium", "balanced (default)"),
        (TL::High, "high", "deep reasoning"),
        (TL::XHigh, "max", "maximum reasoning (slowest)"),
    ];
    let mut items: Vec<(String, String)> = vec![("Back to menu".to_string(), "@menu".to_string())];
    items.extend(levels.iter().map(|(level, name, desc)| {
        let mark = if *level == current { "*" } else { " " };
        (
            format!("{mark} {name} - {desc}"),
            format!("/thinking {name}"),
        )
    }));
    Selector {
        title: "Reasoning effort (thinking level)".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    }
}

fn session_selector(config: &AppConfig) -> Selector {
    let mut items = vec![
        ("Back to menu".to_string(), "@menu".to_string()),
        ("Session actions".to_string(), String::new()),
        ("  Start a new session".to_string(), "/new".to_string()),
        ("Saved sessions".to_string(), String::new()),
    ];
    items.extend(
        SessionStore::list_session_lines(config)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let _id = fields.next()?;
                let name = fields.next().unwrap_or("-");
                let count = fields.next().unwrap_or("").replace(" messages", " msg");
                let created = fields.next().unwrap_or("");
                let path = line
                    .split('\t')
                    .next_back()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let ago = session_ago(&path);
                let label = if ago.is_empty() {
                    format!("{name}  -  {count}  -  {}", short_when(created))
                } else {
                    format!("{name}  -  {count}  -  {}  -  {ago}", short_when(created))
                };
                Some((label, format!("/resume {path}")))
            }),
    );
    Selector {
        title: "Resume a session (or start new)".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    }
}

fn command_menu() -> Selector {
    let items = [
        ("Close menu", "@close"),
        ("Setup", ""),
        ("  Settings dashboard (all knobs)", "/settings"),
        ("  Login provider", "/login"),
        ("  Add MCP server", "/mcp add "),
        ("  Claude/Codex interop on/off", "/interop "),
        ("  New skill (scaffold SKILL.md)", "/skill new "),
        ("  Computer use on/off (desktop control)", "/computer "),
        ("  Accounts & usage", "/accounts"),
        ("  Upgrade to latest release", "/update"),
        ("  Switch model", "/model"),
        ("  Thinking level", "/thinking"),
        ("  Harness role models", "/roles"),
        ("  Persona picker", "@personas"),
        ("  Change codebase folder", "/cd"),
        ("Work", ""),
        ("  New task", "/new"),
        ("  Resume session", "/resume"),
        ("  Sessions", "/sessions"),
        ("  Plan only", "/plan "),
        ("  Harness: plan + build + review", "/harness "),
        ("  Bugfix: find + fix + verify", "/bugfix "),
        ("  Review with second pass", "/review "),
        ("  Loop until complete", "/loop "),
        ("  Batch across files", "/batch "),
        ("  Multi-task in parallel", "/orchestrate "),
        ("Tools", ""),
        ("  Links in conversation", "/links"),
        ("  Codebase tree", "/files"),
        ("  Worktree branch", "/worktree "),
        ("  Review uncommitted changes", "/lens"),
        ("  Restore changed files", "/restore"),
        ("  Self-benchmark", "/bench"),
        ("  Land changes", "/land "),
        ("  Project wiki", "/wiki"),
        ("Advanced / misc", ""),
        ("  Standing goal", "/goal "),
        ("  Self-improve project", "/improve "),
        ("  Auto-upgrade project", "/autoimprove "),
        ("  Help", "/help"),
        ("  Exit", "/exit"),
    ]
    .into_iter()
    .map(|(label, command)| (label.to_string(), command.to_string()))
    .collect();
    let mut selector = Selector {
        title: "BBARIT menu".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    };
    selector.cursor = selector.first_selectable();
    selector
}

/// One-pick login: choose a provider; OAuth providers start the browser/device
/// flow, API-key providers prefill the input so you can paste the key.
/// Providers that support browser (OAuth) sign-in. They keep the full login
/// picker so the user can choose browser vs. API key; every other provider is
/// key-only and jumps straight to the masked key prompt.
fn provider_supports_browser_login(provider: &str) -> bool {
    matches!(provider, "anthropic" | "openai-codex" | "github-copilot")
}

fn login_selector(registry: &Registry) -> Selector {
    let curated = [
        ("Back to menu", "@menu"),
        ("Logged-in accounts & usage", "/accounts"),
        ("Browser login (multi-account)", ""),
        ("  Anthropic (Claude) — disabled", "/login anthropic"),
        ("  OpenAI Codex (ChatGPT)", "/login openai-codex"),
        ("  GitHub Copilot device login", "/login github-copilot"),
        ("API key", ""),
        // Explicit `api-key` form: the trailing space opens the masked input
        // overlay, and the keyword survives login()'s OAuth-provider guard.
        ("  Anthropic (API key)", "/login anthropic api-key "),
        // A raw key can't drive the Codex OAuth backend (it needs a ChatGPT
        // account id from the OAuth token), so OpenAI keys go to "openai".
        ("  OpenAI (ChatGPT/Codex API key)", "/login openai "),
        (
            "  GitHub Copilot (API key)",
            "/login github-copilot api-key ",
        ),
        ("  Z.AI / GLM", "/login zai "),
        ("  Qwen (DashScope)", "/login dashscope "),
        ("  Google Gemini", "/login google "),
        ("  OpenRouter", "/login openrouter "),
        ("  DeepSeek", "/login deepseek "),
        ("  xAI (Grok)", "/login xai "),
        ("  Kimi Code (Kimi For Coding)", "/login kimi-coding "),
        ("  Moonshot AI (Kimi)", "/login moonshotai "),
    ];
    let curated_ids = [
        "anthropic",
        "openai-codex",
        "github-copilot",
        "openai",
        "zai",
        "dashscope",
        "google",
        "openrouter",
        "deepseek",
        "xai",
        "kimi-coding",
        "moonshotai",
    ];
    let mut items: Vec<(String, String)> = curated
        .into_iter()
        .map(|(label, command)| (label.to_string(), command.to_string()))
        .collect();
    // Every other catalog provider gets an API-key entry too — /login works
    // for all of them, so the picker must not hide the rest of the catalog.
    let mut others: Vec<(String, String)> = registry
        .providers()
        .filter(|provider| !curated_ids.contains(&provider.id.as_str()) && provider.id != "ollama")
        .map(|provider| {
            (
                format!("  {}", provider.name),
                format!("/login {} ", provider.id),
            )
        })
        .collect();
    others.sort();
    if !others.is_empty() {
        items.push(("API key (more providers)".to_string(), String::new()));
        items.extend(others);
    }
    Selector {
        title: "Connect a provider".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled: Default::default(),
    }
}

/// One cell of the accounts dashboard: a colored usage gauge (bar filled to
/// used%, text overlaid) like TeamClaude's session/weekly columns.
fn usage_gauge(window: Option<&crate::usage::UsageWindow>, width: usize) -> Vec<Span<'static>> {
    let Some(window) = window else {
        return vec![Span::styled(
            format!("{:-^width$}", ""),
            Style::new().fg(Color::DarkGray),
        )];
    };
    let pct = window.used_percent.clamp(0.0, 100.0);
    let reset = window
        .resets_at
        .map(crate::usage::remaining_hint)
        .unwrap_or_default();
    let mut text = format!(" {pct:>3.0}% {reset}");
    if text.len() > width {
        text.truncate(width);
    } else {
        text = format!("{text:<width$}");
    }
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let color = if pct >= 90.0 {
        Color::Red
    } else if pct >= 70.0 {
        Color::Yellow
    } else {
        Color::Green
    };
    let (used, left) = text.split_at(filled.min(text.len()));
    vec![
        Span::styled(used.to_string(), Style::new().fg(Color::Black).bg(color)),
        Span::styled(
            left.to_string(),
            Style::new().fg(Color::White).bg(Color::DarkGray),
        ),
    ]
}

/// Accounts & usage dashboard (TeamClaude-style): EVERY stored Claude/Codex
/// account on its own row with status plus colored session (5h) and weekly
/// gauges. ↑↓ + Enter on any row switches to that account instantly and the
/// dashboard stays open, so hopping between accounts is one keystroke.
fn accounts_selector(config: &AppConfig) -> Selector {
    crate::auth::backfill_account_emails(config);
    let providers = [
        ("anthropic", "Claude (Anthropic)"),
        ("openai-codex", "Codex (ChatGPT)"),
    ];
    // One usage call PER ACCOUNT (each with its own token), all in parallel —
    // the picker blocks the UI while it is built.
    let per_provider: Vec<(&str, &str, Vec<crate::auth::AccountInfo>)> = providers
        .iter()
        .map(|(provider, title)| {
            (
                *provider,
                *title,
                crate::auth::list_accounts(config, provider).unwrap_or_default(),
            )
        })
        .collect();
    let mut usage_by_key: std::collections::HashMap<
        String,
        anyhow::Result<crate::usage::ProviderUsage>,
    > = std::collections::HashMap::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = per_provider
            .iter()
            .flat_map(|(_, _, accounts)| accounts.iter())
            .filter(|account| account.oauth)
            .map(|account| {
                let key = account.key.clone();
                (
                    account.key.clone(),
                    scope.spawn(move || crate::usage::fetch_usage_for_key(config, &key)),
                )
            })
            .collect();
        for (key, handle) in handles {
            if let Ok(result) = handle.join() {
                usage_by_key.insert(key, result);
            }
        }
    });

    let mut items: Vec<(String, String)> = vec![("Back to menu".to_string(), "@menu".to_string())];
    let mut styled: std::collections::HashMap<usize, Line<'static>> =
        std::collections::HashMap::new();
    for (provider, title, accounts) in &per_provider {
        items.push((
            format!("{title} — {} account(s)", accounts.len()),
            String::new(),
        ));
        for account in accounts {
            let usage = usage_by_key.get(&account.key);
            let session = usage
                .and_then(|result| result.as_ref().ok())
                .and_then(|usage| crate::usage::session_window(usage));
            let week = usage
                .and_then(|result| result.as_ref().ok())
                .and_then(|usage| crate::usage::week_window(usage));
            let throttled = session.is_some_and(|w| w.used_percent >= 99.5)
                || week.is_some_and(|w| w.used_percent >= 99.5);
            let (status, status_color) = if throttled {
                ("throttled", Color::Yellow)
            } else if account.active {
                ("active", Color::Green)
            } else if !account.oauth {
                ("api-key", Color::DarkGray)
            } else {
                ("ready", Color::Gray)
            };
            // Plain label mirrors the row so type-to-filter still works.
            let plain = format!(
                "{} {}  {}  5h {}  wk {}",
                if account.active { "✓" } else { " " },
                account.label,
                status,
                session
                    .map(|w| format!("{:.0}%", w.used_percent))
                    .unwrap_or_else(|| "-".to_string()),
                week.map(|w| format!("{:.0}%", w.used_percent))
                    .unwrap_or_else(|| "-".to_string()),
            );
            let mut spans = vec![
                Span::styled(
                    format!(
                        "{} {:<24}",
                        if account.active { "✓" } else { " " },
                        truncate_label(&account.label, 24)
                    ),
                    if account.active {
                        Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(Color::Reset)
                    },
                ),
                Span::styled(format!("{status:<10}"), Style::new().fg(status_color)),
            ];
            match usage {
                Some(Err(error)) => {
                    let mut reason = format!("{error:#}");
                    reason.truncate(48);
                    spans.push(Span::styled(reason, Style::new().fg(Color::DarkGray)));
                }
                _ => {
                    spans.push(Span::styled(
                        "Ses ".to_string(),
                        Style::new().fg(Color::Gray),
                    ));
                    spans.extend(usage_gauge(session, 18));
                    spans.push(Span::styled(
                        "  Wk ".to_string(),
                        Style::new().fg(Color::Gray),
                    ));
                    spans.extend(usage_gauge(week, 18));
                }
            }
            let command = if account.active {
                "/accounts".to_string()
            } else {
                format!("/accounts use {}", account.key)
            };
            styled.insert(items.len(), Line::from(spans));
            items.push((plain, command));
        }
        if accounts.is_empty() {
            items.push((
                "  Not logged in — connect account".to_string(),
                format!("/login {provider}"),
            ));
        } else {
            items.push((
                "  ＋ Add another account (browser login)".to_string(),
                format!("/login {provider}"),
            ));
            items.push((
                "  ⏏ Sign out active account".to_string(),
                format!("/accounts logout {provider}"),
            ));
        }
    }
    let mut selector = Selector {
        title: "Accounts & usage — Enter switches account".to_string(),
        items,
        filter: String::new(),
        cursor: 0,
        styled,
    };
    // Start on the first ACCOUNT row (not "Back to menu") so switching is
    // pure ↑↓ + Enter from the moment the dashboard opens.
    selector.cursor = selector
        .styled
        .keys()
        .min()
        .copied()
        .unwrap_or_else(|| selector.first_selectable());
    selector
}

fn truncate_label(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        label.to_string()
    } else {
        let mut out: String = label.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// A single-field input overlay shown IN the menu, so commands that need a value
/// (an API key, a task description) are typed inside the menu instead of dumping
/// the command back into the message box — the flow users found annoying.
struct Prompt {
    title: String,
    /// Command prefix (with trailing space) the typed value is appended to.
    prefix: String,
    value: String,
    /// Hide the typed characters and never echo them into the transcript
    /// (API keys).
    masked: bool,
}

impl Prompt {
    fn for_command(command: &str) -> Prompt {
        let trimmed = command.trim_end();
        let (title, masked) = if trimmed == "/login" || trimmed.starts_with("/login ") {
            let provider = trimmed.strip_prefix("/login").unwrap_or("").trim();
            // The explicit `api-key` kind is plumbing, not the provider name.
            let provider = provider.strip_suffix(" api-key").unwrap_or(provider);
            (
                format!("{} · paste API key", pretty_provider(provider)),
                true,
            )
        } else {
            (prompt_title(trimmed), false)
        };
        Prompt {
            title,
            prefix: command.to_string(),
            value: String::new(),
            masked,
        }
    }
}

fn pretty_provider(provider: &str) -> String {
    match provider {
        "openai" => "OpenAI".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "zai" => "Z.AI / GLM".to_string(),
        "dashscope" => "Qwen (DashScope)".to_string(),
        "google" => "Google Gemini".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "xai" => "xAI (Grok)".to_string(),
        "kimi-coding" => "Kimi Code".to_string(),
        "moonshotai" => "Moonshot AI".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "github-copilot" => "GitHub Copilot".to_string(),
        "" => "Provider".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Provider".to_string(),
            }
        }
    }
}

fn prompt_title(prefix: &str) -> String {
    match prefix {
        "/harness" => "Harness — plan → build → review. Describe the task",
        "/plan" => "Plan only. Describe the task",
        "/bugfix" => "Bugfix. Describe the symptom",
        "/review" => "Review with a second pass. Describe the task",
        "/loop" => "Loop until complete. Describe the task",
        "/batch" => "Batch across files. Describe the change",
        "/orchestrate" => "Parallel tasks — separate with |",
        "/goal" => "Standing goal to keep working toward",
        "/improve" => "Self-improve. What to improve",
        "/autoimprove" => "Auto-upgrade rounds 1-10 (default 3)",
        "/worktree" => "Worktree branch name (or: merge/land · off/exit)",
        "/land" => "Land changes. Commit message",
        "/wiki" => "Wiki search query",
        _ => "Enter a value",
    }
    .to_string()
}

/// Persist input history — as JSON Lines (one line = one input) in the session directory,
/// so Up/Down recall survives a restart. Multi-line input is packed onto one line with JSON
/// escaping. session.rs owns the filename — session scanning excludes this file there
/// so it isn't mistaken for a session.
const INPUT_HISTORY_MAX: usize = 500;

fn input_history_path(config: &AppConfig) -> PathBuf {
    config.session_dir.join(INPUT_HISTORY_FILE)
}

fn load_input_history(config: &AppConfig) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(input_history_path(config)) else {
        return Vec::new();
    };
    let mut items: Vec<String> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<String>(line).ok())
        .filter(|line| !line.trim().is_empty())
        .collect();
    if items.len() > INPUT_HISTORY_MAX {
        items.drain(..items.len() - INPUT_HISTORY_MAX);
        // Trim the file too to prevent unbounded growth (best-effort).
        let rewritten: String = items
            .iter()
            .filter_map(|line| serde_json::to_string(line).ok())
            .map(|line| line + "\n")
            .collect();
        let _ = std::fs::write(input_history_path(config), rewritten);
    }
    items
}

fn append_input_history(config: &AppConfig, line: &str) {
    let Ok(encoded) = serde_json::to_string(line) else {
        return;
    };
    let path = input_history_path(config);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{encoded}");
    }
}

struct App {
    input: String,
    scroll: u16,
    follow: bool,
    /// Max scroll offset of the last rendered frame — the anchor for scrolling
    /// up out of follow mode (without it, PageUp from the live tail jumped to
    /// the very top because `scroll` was still 0).
    last_max_scroll: u16,
    status: String,
    entries: Vec<Entry>,
    selector: Option<Selector>,
    /// In-menu input overlay for commands that need a typed value.
    prompt: Option<Prompt>,
    /// Messages typed while a turn was running, processed after it finishes.
    queued: Vec<String>,
    /// Show full tool output (Ctrl+T) instead of folding long results.
    tools_expanded: bool,
    /// True while a turn is running (drives the bright "working" status bar).
    working: bool,
    /// Animated working label shown in the top bar during a turn.
    working_text: String,
    /// Centered top title (session name or project).
    title: String,
    /// Session tabs for the current project: (label, id). Active = active_session.
    tabs: Vec<(String, String)>,
    active_session: String,
    exit: bool,
    /// Submitted inputs, oldest first — recalled with Up/Down in the composer.
    history: Vec<String>,
    /// Browse position in `history` (None = editing a fresh line).
    history_pos: Option<usize>,
    /// Transcript drag-selection as ABSOLUTE wrapped-row indices (anchor, cursor).
    /// Mouse capture is on (wheel scroll), so the host terminal cannot make its own
    /// selection — the TUI provides drag-select + copy itself.
    mouse_sel: Option<(usize, usize)>,
    /// True once a Drag event arrived for the current selection (a plain click
    /// without movement deselects instead of copying).
    mouse_sel_dragged: bool,
    /// Transcript rect + effective scroll of the LAST rendered frame — maps a
    /// mouse (row, col) to an absolute wrapped-row index.
    last_transcript_area: Rect,
    last_scroll: u16,
    /// First Ctrl+C press time: Ctrl+C copies the selection (if any) or clears the
    /// input, and only a second press within the window exits. Copy muscle memory
    /// (Ctrl+C) must never kill the agent outright.
    ctrl_c_at: Option<std::time::Instant>,
}

impl App {
    /// Up: recall the previous input (shell-history style). Shared by the idle composer and the
    /// in-turn input loop.
    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let pos = match self.history_pos {
            None => self.history.len() - 1,
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.history_pos = Some(pos);
        self.input = self.history[pos].clone();
    }

    /// Down: move to the next input; past the end, return to a blank new line.
    fn history_next(&mut self) {
        match self.history_pos {
            Some(p) if p + 1 < self.history.len() => {
                self.history_pos = Some(p + 1);
                self.input = self.history[p + 1].clone();
            }
            Some(_) => {
                self.history_pos = None;
                self.input.clear();
            }
            None => {}
        }
    }

    /// Record a submitted input to history (memory + file). Skipped if identical to the previous entry.
    fn record_history(&mut self, config: &AppConfig, line: &str) {
        self.history_pos = None;
        if line.is_empty() || self.history.last().map(String::as_str) == Some(line) {
            return;
        }
        self.history.push(line.to_string());
        append_input_history(config, line);
    }

    /// Scroll the transcript up (into history), leaving follow mode from the
    /// current bottom anchor rather than jumping to the top.
    fn scroll_up_by(&mut self, lines: u16) {
        let base = if self.follow {
            self.last_max_scroll
        } else {
            self.scroll
        };
        self.follow = false;
        self.scroll = base.saturating_sub(lines);
    }

    /// Scroll the transcript down; hitting the bottom re-enables follow so new
    /// output keeps streaming into view.
    fn scroll_down_by(&mut self, lines: u16) {
        if self.follow {
            return;
        }
        self.scroll = self.scroll.saturating_add(lines);
        if self.scroll >= self.last_max_scroll {
            self.scroll = self.last_max_scroll;
            self.follow = true;
        }
    }

    /// Map a terminal cell (column,row) to an ABSOLUTE wrapped transcript row
    /// using the last rendered frame's rect + scroll. None outside the transcript.
    fn transcript_row_at(&self, column: u16, row: u16) -> Option<usize> {
        let a = self.last_transcript_area;
        if a.width == 0 || a.height == 0 {
            return None;
        }
        if column < a.x || column >= a.x.saturating_add(a.width) {
            return None;
        }
        if row < a.y || row >= a.y.saturating_add(a.height) {
            return None;
        }
        Some(self.last_scroll as usize + (row - a.y) as usize)
    }

    /// Like transcript_row_at but clamped into the transcript's vertical range —
    /// dragging past the top/bottom edge keeps extending to the first/last visible row.
    fn transcript_row_clamped(&self, row: u16) -> usize {
        let a = self.last_transcript_area;
        let top = a.y;
        let bottom = a.y.saturating_add(a.height.max(1)).saturating_sub(1);
        let r = row.clamp(top, bottom);
        self.last_scroll as usize + (r - top) as usize
    }
}

/// Plain text of one rendered transcript row (spans concatenated, right-trimmed).
fn line_plain_text(line: &Line<'_>) -> String {
    let mut s = String::new();
    for span in &line.spans {
        s.push_str(span.content.as_ref());
    }
    s.trim_end().to_string()
}

/// Rows lo..=hi (absolute wrapped-row indices, clamped) as one plain-text block.
fn transcript_selection_text(lines: &[Line<'_>], lo: usize, hi: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let lo = lo.min(lines.len() - 1);
    let hi = hi.min(lines.len() - 1);
    lines[lo..=hi]
        .iter()
        .map(line_plain_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Copy the active drag-selection to the system clipboard; reports via the status line.
fn copy_transcript_selection(app: &mut App, partial: Option<&str>) {
    let Some((a, b)) = app.mouse_sel else { return };
    let (lo, hi) = (a.min(b), a.max(b));
    let width = app.last_transcript_area.width.max(1) as usize;
    let mut lines = transcript_lines(&app.entries, partial, width, app.tools_expanded);
    // Match the render's absolute row space: a short transcript is drawn
    // bottom-aligned with blank pad rows on top, and selection indices come
    // from that padded space.
    let visible_rows = app.last_transcript_area.height as usize;
    if app.follow && visible_rows > 0 && lines.len() < visible_rows {
        let pad = visible_rows - lines.len();
        let mut padded: Vec<Line<'static>> = Vec::with_capacity(visible_rows);
        padded.extend((0..pad).map(|_| Line::raw("")));
        padded.append(&mut lines);
        lines = padded;
    }
    let text = transcript_selection_text(&lines, lo, hi);
    if text.trim().is_empty() {
        app.status = "copy: empty selection".to_string();
        return;
    }
    let rows = hi.saturating_sub(lo) + 1;
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
        Ok(()) => app.status = format!("copied {rows} row(s)"),
        Err(e) => app.status = format!("copy failed: {e}"),
    }
}

/// Transcript mouse handling shared by the idle and streaming event loops.
/// Mouse capture is on for wheel scroll, which prevents the HOST terminal from
/// making a selection — so the TUI provides its own: Left down/drag selects
/// whole transcript rows (REVERSED highlight), release copies them to the
/// clipboard. A plain click (no drag) just deselects.
fn handle_transcript_mouse(app: &mut App, mouse: event::MouseEvent, partial: Option<&str>) {
    use ratatui::crossterm::event::MouseButton;
    match mouse.kind {
        MouseEventKind::ScrollUp => app.scroll_up_by(3),
        MouseEventKind::ScrollDown => app.scroll_down_by(3),
        MouseEventKind::Down(MouseButton::Left) => {
            app.mouse_sel_dragged = false;
            app.mouse_sel = app
                .transcript_row_at(mouse.column, mouse.row)
                .map(|r| (r, r));
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.mouse_sel.is_some() {
                let r = app.transcript_row_clamped(mouse.row);
                if let Some(sel) = app.mouse_sel.as_mut() {
                    sel.1 = r;
                }
                app.mouse_sel_dragged = true;
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.mouse_sel.is_some() && app.mouse_sel_dragged {
                copy_transcript_selection(app, partial);
            } else {
                app.mouse_sel = None;
            }
        }
        _ => {}
    }
}

/// Refresh the session-tab bar from the current project's sessions.
fn refresh_tabs(app: &mut App, store: &SessionStore, config: &AppConfig) {
    app.active_session = store.session().id.clone();
    app.tabs = SessionStore::list_session_lines(config)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next()?.to_string();
            let name = fields.next().unwrap_or("");
            let label: String = name.chars().take(14).collect();
            Some((
                if label.trim().is_empty() {
                    id.chars().take(6).collect()
                } else {
                    label
                },
                id,
            ))
        })
        .take(9)
        .collect();
}

pub fn run_interactive(
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    initial: Option<String>,
) -> Result<()> {
    // Non-blocking: check for a newer release in the background; the splash
    // shows a hint once it lands, never delaying startup.
    crate::update::spawn_startup_check();
    // Disk hygiene: keep only the 30 most recent sessions (the open one is safe).
    crate::session::prune_old_sessions(config, 30, &store.session().id);
    // A /goal is scoped to its session; starting a new interactive session drops
    // any goal carried over so an old one-off goal doesn't silently steer this one.
    crate::commands::reset_goal_for_new_session(config);
    // Populate the wiki panel from the project's wiki db.
    crate::commands::refresh_wiki_panel(config);
    // ratatui needs a real terminal; when stdin/stdout is piped (scripts,
    // smoke tests) fall back to a line-based REPL so it stays scriptable.
    if !atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stdout) {
        return run_line_repl(store, registry, config, initial);
    }
    // Force streaming in the interactive TUI: tokens show live AND Esc can cancel
    // mid-response (the SSE reader checks for cancellation per line). Without it a
    // big reply must finish generating before cancel is honored — the "cancel feels slow"
    // symptom.
    config.stream = true;
    // While the TUI draws, redirect stderr to a log file (the agent dir's tui-stderr.log).
    // Prevents the bug where eprintln! printed onto the raw-mode screen, wrote error text into
    // the input box, and hid the bottom UI. Restored when the guard drops (normal or panic).
    let _stderr_guard = StderrRedirectGuard::install();
    enable_raw_mode()?;
    // Mouse capture: wheel scrolls the transcript. (In the embedded terminal
    // xterm.js still produces selections while mouse tracking is on, so
    // copy-on-select keeps working.)
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    // On panic, restore the terminal first so the error is visible and the shell
    // is usable (otherwise a panic leaves a raw/alt-screen "dead" terminal).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore stderr first so panic messages aren't hidden in the log file (the hook runs before Drop).
        restore_stderr_for_panic();
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            DisableMouseCapture
        );
        default_hook(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = run_app(&mut terminal, store, registry, config, initial);
    disable_raw_mode().ok();
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();
    result.map_err(Into::into)
}

/// Commands that only drive the interactive TUI screen (pickers, transcript,
/// worktree/codebase state on App). The line REPL accepts them with a short
/// note instead of "Unknown command" plus the full help dump.
fn is_interactive_only_command(line: &str) -> bool {
    matches!(
        line.split_whitespace().next().unwrap_or(""),
        "/clear"
            | "/cls"
            | "/menu"
            | "/links"
            | "/personas"
            | "/setup"
            | "/folder"
            | "/codebase"
            | "/cd"
            | "/worktree"
    )
}

/// Line-based REPL used when stdin/stdout is not a terminal: read a command or
/// prompt per line and print the result. Keeps `--tui` scriptable.
fn run_line_repl(
    store: &mut SessionStore,
    registry: &Registry,
    config: &AppConfig,
    initial: Option<String>,
) -> Result<()> {
    use std::io::BufRead;
    if let Some(input) = initial {
        match handle_input(store, registry, config, &input) {
            Ok(output) => println!("{output}"),
            Err(error) => println!("Error: {error:#}"),
        }
    }
    for line in io::stdin().lock().lines() {
        let line = line?;
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" {
            break;
        }
        if is_interactive_only_command(input) {
            println!("(interactive-only — available in the TUI, ignored here)");
            continue;
        }
        match handle_input(store, registry, config, input) {
            Ok(output) => println!("{output}"),
            Err(error) => println!("Error: {error:#}"),
        }
    }
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    initial: Option<String>,
) -> io::Result<()> {
    let mut entries = seed_entries(store);
    // Startup splash: lead with the BBARIT AGENT logo so every terminal opens
    // with the brand, plus version/platform so "what am I running?" needs no
    // extra command.
    let splash = format!(
        "{BBARIT_LOGO}\n v{} ({})  ·  MIT open source",
        env!("CARGO_PKG_VERSION"),
        crate::update::target_key().unwrap_or(std::env::consts::OS),
    );
    let splash = match crate::update::available_update() {
        Some(version) => format!(
            "{splash}\n \u{2b06} Update available: v{version} \u{2014} run  bbarit-oss --upgrade  (or /update)"
        ),
        None => splash,
    };
    entries.insert(0, Entry::new(Kind::Banner, splash));
    // On a fresh session, point out that earlier sessions can be resumed.
    if entries.len() <= 1 {
        let current = store.session().id.clone();
        let others = SessionStore::list_session_lines(config)
            .unwrap_or_default()
            .into_iter()
            .filter(|line| !line.contains(&current))
            .count();
        if others > 0 {
            entries.push(Entry::new(
                Kind::System,
                format!(
                    "💬 {others} earlier session(s) here — /resume to continue the latest, \
                     or /sessions to pick one."
                ),
            ));
        }
    }
    // First-run onboarding: a brand-new user with no credentials gets the login
    // picker on open (plus a one-line welcome) instead of an auth error on their
    // first message.
    let needs_login = !crate::auth::has_any_login(config);
    if needs_login {
        entries.push(Entry::new(
            Kind::System,
            "👋 Welcome to BBARIT OSS. You're not signed in yet — pick a provider below to \
             sign in, or press Esc and run /login anytime."
                .to_string(),
        ));
    }
    let mut app = App {
        input: String::new(),
        scroll: 0,
        follow: true,
        last_max_scroll: 0,
        status: status_line(store, registry, config),
        entries,
        selector: if needs_login {
            Some(login_selector(registry))
        } else {
            None
        },
        prompt: None,
        queued: Vec::new(),
        tools_expanded: true,
        working: false,
        working_text: String::new(),
        title: title_line(store, config),
        tabs: Vec::new(),
        active_session: store.session().id.clone(),
        exit: false,
        history: load_input_history(config),
        history_pos: None,
        mouse_sel: None,
        mouse_sel_dragged: false,
        last_transcript_area: Rect::new(0, 0, 0, 0),
        last_scroll: 0,
        ctrl_c_at: None,
    };
    refresh_tabs(&mut app, store, config);
    if let Some(input) = initial {
        run_turn(terminal, &mut app, store, registry, config, &input)?;
    }
    // The startup update check runs in the background and usually lands *after*
    // the splash was built (a static entry), so the "update available" hint would
    // never appear. Surface it on the first frame after the check arrives.
    let mut update_hint_shown = crate::update::available_update().is_some();
    while !app.exit {
        if !update_hint_shown && let Some(version) = crate::update::available_update() {
            app.entries.push(Entry::new(
                Kind::System,
                format!("⬆ Update available: v{version} — run  bbarit-oss --upgrade  (or /update)"),
            ));
            update_hint_shown = true;
        }
        terminal.draw(|frame| render(frame, &mut app, None))?;
        // Drain the whole queued burst before the next redraw: fast typing, IME
        // syllables, and multi-character PTY writes (persona briefs, relayed
        // commands) arrive as one key event per character, so handling a
        // single event per frame made input crawl in one-character steps.
        for event in next_events(Duration::from_millis(150))? {
            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key(
                        terminal,
                        &mut app,
                        store,
                        registry,
                        config,
                        key.code,
                        key.modifiers,
                    )?;
                }
                // Wheel scrolls; Left down/drag/up drag-selects transcript rows
                // and copies them on release (mouse capture blocks host selection).
                Event::Mouse(mouse) => handle_transcript_mouse(&mut app, mouse, None),
                Event::Paste(text) => {
                    // Hosts (including xterm.js) send bracketed-paste line breaks as CR;
                    // the composer treats only '\n' as a line break — without this a
                    // multi-line paste collapsed into one clipped line.
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    // Route a paste into whatever overlay is open so it doesn't
                    // land in the hidden composer behind the popup: an input
                    // prompt (e.g. an API key), else a selector's filter (e.g.
                    // pasting a model name to narrow the picker).
                    if let Some(prompt) = app.prompt.as_mut() {
                        prompt.value.push_str(text.trim());
                    } else if let Some(sel) = app.selector.as_mut() {
                        sel.filter.push_str(text.trim());
                        sel.cursor = sel.first_selectable();
                    } else {
                        app.input.push_str(&text);
                    }
                }
                _ => {}
            }
            if app.exit {
                break;
            }
        }
    }
    Ok(())
}

/// Reassembles a bracketed paste that Windows ConPTY delivered as individual
/// key events. The frontend wraps pastes in ESC[200~ … ESC[201~; crossterm
/// only reassembles that into Event::Paste on Unix. Without this, every '\r'
/// in a pasted block submits a line on its own and the markers leak into the
/// input as garbage (and a paste during streaming looks like Esc = cancel).
#[derive(Default)]
enum PasteReassembly {
    #[default]
    Idle,
    /// Saw Esc and `matched` chars of "[200~" so far; `held` are the consumed
    /// events to replay verbatim if this turns out not to be a paste marker.
    Opening { held: Vec<Event>, matched: usize },
    /// Inside the paste body. `closing` counts progress through Esc+"[201~".
    Pasting { text: String, closing: usize },
}

const PASTE_OPEN: [char; 5] = ['[', '2', '0', '0', '~'];
const PASTE_CLOSE: [char; 5] = ['[', '2', '0', '1', '~'];

impl PasteReassembly {
    fn is_active(&self) -> bool {
        !matches!(self, PasteReassembly::Idle)
    }

    /// Feed one event; returns events ready for normal handling (possibly a
    /// synthesized Event::Paste, possibly replayed literal keys on mismatch).
    fn feed(&mut self, event: Event) -> Vec<Event> {
        let key = match &event {
            Event::Key(key) if key.kind == KeyEventKind::Press => Some(key.code),
            Event::Key(_) => None,
            // Mouse/resize during a burst: pass through, keep our state.
            _ if self.is_active() => return vec![event],
            _ => None,
        };
        match self {
            PasteReassembly::Idle => {
                if key == Some(KeyCode::Esc) || key == Some(KeyCode::Char('\x1b')) {
                    *self = PasteReassembly::Opening {
                        held: vec![event],
                        matched: 0,
                    };
                    Vec::new()
                } else {
                    vec![event]
                }
            }
            PasteReassembly::Opening { held, matched } => {
                if key == Some(KeyCode::Char(PASTE_OPEN[*matched])) {
                    held.push(event);
                    *matched += 1;
                    if *matched == PASTE_OPEN.len() {
                        *self = PasteReassembly::Pasting {
                            text: String::new(),
                            closing: 0,
                        };
                    }
                    Vec::new()
                } else if key.is_none() {
                    // Key release/repeat between marker chars: swallow it.
                    Vec::new()
                } else {
                    // Not a paste marker: replay everything as literal keys.
                    let mut out = std::mem::take(held);
                    out.push(event);
                    *self = PasteReassembly::Idle;
                    out
                }
            }
            PasteReassembly::Pasting { text, closing } => {
                let Some(code) = key else { return Vec::new() };
                if code == KeyCode::Esc || code == KeyCode::Char('\x1b') {
                    // An Esc + marker chars swallowed by a previous half-match
                    // were literal content; restore them before restarting.
                    if *closing >= 1 {
                        text.push('\x1b');
                        text.extend(PASTE_CLOSE.iter().take(*closing - 1));
                    }
                    *closing = 1;
                    return Vec::new();
                }
                if *closing >= 1 {
                    if code == KeyCode::Char(PASTE_CLOSE[*closing - 1]) {
                        *closing += 1;
                        if *closing == PASTE_CLOSE.len() + 1 {
                            let done = Event::Paste(std::mem::take(text));
                            *self = PasteReassembly::Idle;
                            return vec![done];
                        }
                        return Vec::new();
                    }
                    // Half-matched closing marker was literal content.
                    text.push('\x1b');
                    text.extend(PASTE_CLOSE.iter().take(*closing - 1));
                    *closing = 0;
                }
                match code {
                    KeyCode::Enter => text.push('\n'),
                    KeyCode::Tab => text.push('\t'),
                    KeyCode::Char(c) => text.push(c),
                    _ => {}
                }
                Vec::new()
            }
        }
    }

    /// The queue went quiet: resolve any half-open state. A lone Esc replays
    /// as a real Esc; an unterminated paste is emitted as-is (best recovery).
    fn flush(&mut self) -> Vec<Event> {
        match std::mem::take(self) {
            PasteReassembly::Idle => Vec::new(),
            PasteReassembly::Opening { held, .. } => held,
            PasteReassembly::Pasting { text, .. } => vec![Event::Paste(text)],
        }
    }
}

/// Wait up to `first_wait` for input, then drain the whole queued burst
/// (reassembling bracketed pastes) so one redraw covers many characters.
fn next_events(first_wait: Duration) -> io::Result<Vec<Event>> {
    let mut out = Vec::new();
    if !event::poll(first_wait)? {
        return Ok(out);
    }
    let mut reasm = PasteReassembly::default();
    loop {
        out.extend(reasm.feed(event::read()?));
        // Mid-paste, bridge the frontend's chunked-write gaps (~5ms between
        // 4KB chunks); otherwise only drain what is already queued.
        let wait = if reasm.is_active() {
            Duration::from_millis(60)
        } else {
            Duration::ZERO
        };
        if !event::poll(wait)? {
            out.extend(reasm.flush());
            return Ok(out);
        }
    }
}

fn handle_key(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> io::Result<()> {
    // Ctrl+C: copy the transcript selection if one exists (terminal muscle memory),
    // otherwise clear the input; only a SECOND press within 1.5s exits. A single
    // Ctrl+C must never kill the agent — "select text, press Ctrl+C to copy" was
    // silently quitting whole sessions.
    if let (KeyCode::Char('c'), KeyModifiers::CONTROL) = (code, modifiers) {
        if app.mouse_sel.is_some() {
            copy_transcript_selection(app, None);
            return Ok(());
        }
        let now = std::time::Instant::now();
        if app
            .ctrl_c_at
            .is_some_and(|t| now.duration_since(t) < Duration::from_millis(1500))
        {
            app.exit = true;
            return Ok(());
        }
        app.ctrl_c_at = Some(now);
        app.input.clear();
        app.history_pos = None;
        app.status = "Ctrl+C again to exit".to_string();
        return Ok(());
    }
    app.ctrl_c_at = None;

    // In-menu input overlay (API key / task text). Modal: handle it and return.
    if let Some(prompt) = app.prompt.as_mut() {
        match code {
            KeyCode::Esc => app.prompt = None,
            KeyCode::Enter => {
                let value = prompt.value.trim().to_string();
                let prefix = prompt.prefix.clone();
                let masked = prompt.masked;
                app.prompt = None;
                if !value.is_empty() {
                    let full = format!("{prefix}{value}");
                    if masked {
                        // Secret (API key): run it quietly and NEVER echo the
                        // value into the transcript.
                        run_command_quiet(app, store, registry, config, &full);
                        app.status = status_line(store, registry, config);
                    } else {
                        dispatch_line(terminal, app, store, registry, config, &full)?;
                    }
                }
            }
            KeyCode::Backspace => {
                prompt.value.pop();
            }
            KeyCode::Char(ch) => prompt.value.push(ch),
            _ => {}
        }
        return Ok(());
    }

    // Selector overlay handling.
    if let Some(selector) = app.selector.as_mut() {
        match code {
            KeyCode::Esc => app.selector = None,
            KeyCode::Up => selector.move_cursor(-1),
            KeyCode::Down => selector.move_cursor(1),
            KeyCode::Backspace => {
                selector.filter.pop();
                selector.cursor = selector.first_selectable();
            }
            KeyCode::Char(ch) => {
                selector.filter.push(ch);
                selector.cursor = selector.first_selectable();
            }
            KeyCode::Enter => {
                let command = selector.selected_command();
                app.selector = None;
                if let Some(command) = command {
                    apply_command(terminal, app, store, registry, config, &command);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Chat mode.
    match (code, modifiers) {
        // Esc/Ctrl+U clear the input rather than quitting (accidental Esc shouldn't
        // kill the session); exit is Ctrl+C or /exit. Ctrl+U is what the host
        // terminal "clear input" button sends.
        // When the input box is empty, Esc clears queued messages — since cancel does not
        // remove the queue, this is the only explicit way to drop it.
        (KeyCode::Esc, _) => {
            if !app.input.is_empty() {
                app.input.clear();
            } else if !app.queued.is_empty() {
                let n = app.queued.len();
                app.queued.clear();
                app.status = format!("{n} queued message(s) discarded");
            }
        }
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.input.clear(),
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
            app.entries.clear();
            app.follow = true;
            app.scroll = 0;
            terminal.clear().ok();
        }
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
            app.tools_expanded = !app.tools_expanded;
        }
        // Alt+Enter/Shift+Enter insert a newline (multiline composer); plain Enter
        // sends. The host terminal sends Shift+Enter as CSI-u (ESC[13;2u) which
        // crossterm parses as Enter+SHIFT — without this arm it submitted the line.
        (KeyCode::Enter, m) if m.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) => {
            app.input.push('\n')
        }
        (KeyCode::Tab, _) => {
            // Complete a slash command if one is being typed, else open the menu.
            if let Some(top) = command_suggestions(&app.input).into_iter().next() {
                app.input = top;
            } else {
                app.selector = Some(command_menu());
            }
        }
        (KeyCode::Enter, _) => {
            let line = app.input.trim().to_string();
            app.input.clear();
            // Record in input history (Up/Down recalls it); skip dupes of the last.
            app.record_history(config, &line);
            dispatch_line(terminal, app, store, registry, config, &line)?;
        }
        (KeyCode::Backspace, _) => {
            app.input.pop();
        }
        (KeyCode::PageUp, _) => app.scroll_up_by(10),
        (KeyCode::PageDown, _) => app.scroll_down_by(10),
        // Up/Down recall previous inputs (shell-style history). Scroll the
        // transcript with PageUp/PageDown.
        (KeyCode::Up, _) => app.history_prev(),
        (KeyCode::Down, _) => app.history_next(),
        // Ctrl+V: if the clipboard has an image, attach it (as @path.png);
        // pasted text arrives via the bracketed-paste Event::Paste path.
        (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
            if let Some(token) = clipboard_image_token() {
                if !app.input.is_empty() && !app.input.ends_with(' ') {
                    app.input.push(' ');
                }
                app.input.push_str(&token);
                app.input.push(' ');
            }
        }
        // Alt+N: new session (new "terminal" for this project).
        (KeyCode::Char('t') | KeyCode::Char('n'), KeyModifiers::ALT) => {
            apply_command(terminal, app, store, registry, config, "/new");
        }
        // Alt+1..9: switch to that session tab.
        (KeyCode::Char(digit @ '1'..='9'), KeyModifiers::ALT) => {
            let index = digit as usize - '1' as usize;
            if let Some((_, id)) = app.tabs.get(index).cloned()
                && id != app.active_session
            {
                apply_command(
                    terminal,
                    app,
                    store,
                    registry,
                    config,
                    &format!("/resume {id}"),
                );
            }
        }
        // Only insert a bare/Shift character. An unhandled Ctrl/Alt/Super chord
        // must NOT leak its letter into the composer (Ctrl+A typing "a", etc.).
        (KeyCode::Char(ch), m)
            if !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            app.input.push(ch);
        }
        _ => {}
    }
    Ok(())
}

/// Commands that replace the loaded session (bare or with an argument). They
/// must reseed the transcript via apply_command; any other path would swap the
/// store under a transcript seeded from the previous session.
fn is_session_switch_command(line: &str) -> bool {
    matches!(
        line.split_whitespace().next().unwrap_or(""),
        "/new" | "/clone" | "/fork" | "/resume" | "/import"
    )
}

/// Apply a command chosen from a selector. Some commands re-open a sub-selector;
/// Dispatch a submitted line (typed + Enter, or completed from an in-menu
/// prompt): slash commands open their picker or run; anything else is an agent
/// turn. Shared so the message box and the menu prompt behave identically.
fn dispatch_line(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    line: &str,
) -> io::Result<()> {
    match line {
        "/exit" | "/quit" => app.exit = true,
        "/model" | "/models" => app.selector = Some(provider_selector(registry, config)),
        "/ollama" => app.selector = Some(model_selector(registry, config, Some("ollama"), "")),
        "/thinking" => {
            let current = crate::commands::current_thinking_level(store, registry, config);
            app.selector = Some(thinking_selector(current));
        }
        "/sessions" => app.selector = Some(session_selector(config)),
        "/login" => app.selector = Some(login_selector(registry)),
        "/accounts" => app.selector = Some(accounts_selector(config)),
        "/roles" => app.selector = Some(roles_selector(config)),
        "/settings" | "/setup" => {
            app.selector = Some(settings_selector(store, registry, config));
        }
        "/persona" | "/personas" => app.selector = Some(persona_selector(config, None)),
        "/links" => app.selector = Some(links_selector(&app.entries)),
        "/menu" => app.selector = Some(command_menu()),
        "/resume" => app.selector = Some(session_selector(config)),
        "/clear" | "/cls" => {
            app.entries.clear();
            app.follow = true;
            app.scroll = 0;
            terminal.clear().ok();
        }
        // Instant session-switch commands reseed the transcript; route them
        // through apply_command so typing them works like the menu. The arg
        // forms must be caught too — falling through to run_turn would swap
        // the store but keep appending to the OLD transcript (run_single_turn's
        // `before` index still counts the previous session's messages).
        path if is_session_switch_command(path) => {
            apply_command(terminal, app, store, registry, config, path)
        }
        "/cd" | "/folder" | "/codebase" => app.selector = Some(folder_selector(config)),
        path if path.starts_with("/cd ")
            || path.starts_with("/folder ")
            || path.starts_with("/codebase ") =>
        {
            let arg = path
                .split_once(' ')
                .map(|(_, rest)| rest.trim())
                .unwrap_or("");
            if arg.is_empty() {
                app.selector = Some(folder_selector(config));
            } else {
                change_codebase(app, store, registry, config, arg);
            }
        }
        // /lens as a NORMAL agent turn: the old sync one-off LLM call blocked
        // the UI thread for its whole duration — the menu looked dead. As a
        // turn it gets the spinner, streaming, and Esc cancellation for free,
        // and the agent fetches the diff itself.
        "/lens" => {
            let _ = run_single_turn(terminal, app, store, registry, config, LENS_TURN_PROMPT);
        }
        "/worktree" => enter_worktree(app, store, registry, config, ""),
        path if path.starts_with("/worktree ") => {
            let arg = path[10..].trim();
            if matches!(arg, "off" | "exit" | "leave") {
                exit_worktree(app, store, registry, config);
            } else if matches!(arg, "merge" | "land") {
                merge_worktree(app, store, registry, config);
            } else {
                enter_worktree(app, store, registry, config, arg);
            }
        }
        "" => {}
        _ => run_turn(terminal, app, store, registry, config, line)?,
    }
    Ok(())
}

/// Run a command synchronously and show only its (sanitized) output as a system
/// note — used for secrets like API keys so the value never lands in the
/// transcript (which `run_turn` would echo as a user message).
fn run_command_quiet(
    app: &mut App,
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    command: &str,
) {
    match handle_input(store, registry, config, command) {
        Ok(output) => {
            if !output.trim().is_empty() {
                app.entries.push(Entry::new(Kind::System, output));
            }
        }
        Err(error) => app
            .entries
            .push(Entry::new(Kind::System, format!("Error: {error:#}"))),
    }
}

/// some prefill the input (need a typed argument); the rest run immediately.
fn apply_command(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    command: &str,
) {
    // Provider chosen in the provider picker → open its model picker.
    if command == "@close" {
        return;
    }
    if command == "@menu" {
        app.selector = Some(command_menu());
        return;
    }
    if let Some(provider) = command.strip_prefix("@prov:") {
        app.selector = Some(model_selector(registry, config, Some(provider), ""));
        return;
    }
    // "← Back" from the model picker → reopen the provider list.
    if command == "@back:providers" {
        app.selector = Some(provider_selector(registry, config));
        return;
    }
    if command == "@back:roles" {
        app.selector = Some(roles_selector(config));
        return;
    }
    if command == "@settings" {
        app.selector = Some(settings_selector(store, registry, config));
        return;
    }
    if command == "@personas" {
        app.selector = Some(persona_selector(config, None));
        return;
    }
    if let Some(role) = command.strip_prefix("@role:") {
        app.selector = Some(if role == "all" {
            harness_model_selector(registry, config, None, false)
        } else {
            harness_model_selector(registry, config, Some(role), false)
        });
        return;
    }
    // Persona for one harness role.
    if let Some(role) = command.strip_prefix("@rolepersona:") {
        app.selector = Some(persona_selector(config, Some(role)));
        return;
    }
    // "Show all models…" from a short role picker → the full catalog.
    if let Some(role) = command.strip_prefix("@roleall:") {
        app.selector = Some(if role == "all" {
            harness_model_selector(registry, config, None, true)
        } else {
            harness_model_selector(registry, config, Some(role), true)
        });
        return;
    }
    // Folder chosen in the codebase picker → switch the working directory.
    if let Some(path) = command.strip_prefix("@cd:") {
        change_codebase(app, store, registry, config, path);
        return;
    }
    // Picked a model from the model list → switch to it, then (only for models
    // that actually reason) immediately offer the thinking-level picker, so you
    // set effort in the same flow. Non-reasoning models skip it.
    if let Some(arg) = command.strip_prefix("/model ")
        && !arg.trim().is_empty()
    {
        match handle_input(store, registry, config, command) {
            Ok(output) => {
                if !output.trim().is_empty() {
                    app.entries.push(Entry::new(Kind::System, output));
                }
            }
            Err(error) => app
                .entries
                .push(Entry::new(Kind::System, format!("Error: {error:#}"))),
        }
        app.status = status_line(store, registry, config);
        if crate::commands::current_model_reasons(store, registry, config) {
            let current = crate::commands::current_thinking_level(store, registry, config);
            app.selector = Some(thinking_selector(current));
        }
        return;
    }
    match command {
        "/thinking" => {
            let current = crate::commands::current_thinking_level(store, registry, config);
            if !crate::commands::current_model_reasons(store, registry, config) {
                app.entries.push(Entry::new(
                    Kind::System,
                    "This model has no thinking level (it doesn't do extra reasoning).".to_string(),
                ));
                return;
            }
            app.selector = Some(thinking_selector(current));
            return;
        }
        "/cd" | "/folder" | "/codebase" => {
            app.selector = Some(folder_selector(config));
            return;
        }
        "/lens" => {
            // Runs as a normal agent turn (spinner + Esc cancel) — the old
            // sync one-off call froze the UI with no feedback.
            let _ = run_single_turn(terminal, app, store, registry, config, LENS_TURN_PROMPT);
            return;
        }
        "/worktree" => {
            enter_worktree(app, store, registry, config, "");
            return;
        }
        path if path.starts_with("/worktree ") => {
            let arg = path[10..].trim();
            if matches!(arg, "off" | "exit" | "leave") {
                exit_worktree(app, store, registry, config);
            } else if matches!(arg, "merge" | "land") {
                merge_worktree(app, store, registry, config);
            } else {
                enter_worktree(app, store, registry, config, arg);
            }
            return;
        }
        "/model" | "/models" => {
            app.selector = Some(provider_selector(registry, config));
            return;
        }
        "/ollama" => {
            app.selector = Some(model_selector(registry, config, Some("ollama"), ""));
            return;
        }
        "/sessions" => {
            app.selector = Some(session_selector(config));
            return;
        }
        "/login" => {
            app.selector = Some(login_selector(registry));
            return;
        }
        "/accounts" => {
            app.selector = Some(accounts_selector(config));
            return;
        }
        "/roles" => {
            app.selector = Some(roles_selector(config));
            return;
        }
        "/settings" | "/setup" => {
            app.selector = Some(settings_selector(store, registry, config));
            return;
        }
        "/persona" | "/personas" => {
            app.selector = Some(persona_selector(config, None));
            return;
        }
        "/links" => {
            app.selector = Some(links_selector(&app.entries));
            return;
        }
        "/exit" | "/quit" => {
            app.exit = true;
            return;
        }
        _ => {}
    }
    // A link row: open it in the browser.
    if command.starts_with("http://") || command.starts_with("https://") {
        let note = match crate::auth::open_url(command) {
            Ok(()) => format!("Opened {command}"),
            Err(error) => format!("Could not open {command}: {error:#}"),
        };
        app.entries.push(Entry::new(Kind::System, note));
        return;
    }
    // A trailing space means the command needs a typed argument. Collect it in an
    // in-menu input overlay instead of dumping the command into the message box
    // (returning to the message line to type there was the confusing part).
    if command.ends_with(' ') {
        app.prompt = Some(Prompt::for_command(command));
        return;
    }
    // OAuth logins open a browser / show a device code: suspend the TUI so the
    // prompts are visible, then resume.
    if command.starts_with("/login ") {
        run_login(terminal, store, registry, config, command);
        app.status = status_line(store, registry, config);
        app.title = title_line(store, config);
        // Multi-login providers land back on the accounts screen so the new
        // account, the active marker, and its usage are visible right away.
        let provider = command.trim_start_matches("/login ").trim();
        let provider = provider.split_whitespace().next().unwrap_or(provider);
        if crate::auth::is_multi_account_provider(provider) {
            app.selector = Some(accounts_selector(config));
        }
        return;
    }
    // Account switch / sign-out: run it, then reopen the accounts screen with
    // the refreshed state.
    if command.starts_with("/accounts ") {
        run_command_quiet(app, store, registry, config, command);
        app.status = status_line(store, registry, config);
        app.selector = Some(accounts_selector(config));
        return;
    }
    // Picking a harness role model: apply it, then RETURN to the roles menu so
    // you can set several roles back-to-back instead of dropping to the chat
    // after every pick. (A model set carries a "provider/id" ref; other /roles
    // subcommands like show/clear/glm just run and return.)
    if let Some(rest) = command.strip_prefix("/roles ") {
        // Model refs carry '/'; persona assignments carry " persona ".
        let sets_assignment = rest.contains('/') || rest.contains(" persona ");
        run_command_quiet(app, store, registry, config, command);
        app.status = status_line(store, registry, config);
        if sets_assignment {
            app.selector = Some(roles_selector(config));
        }
        return;
    }
    match handle_input(store, registry, config, command) {
        Ok(output) => {
            if is_session_switch_command(command) {
                app.entries = seed_entries(store);
            }
            if !output.trim().is_empty() {
                app.entries.push(Entry::new(Kind::System, output));
            }
        }
        Err(error) => app
            .entries
            .push(Entry::new(Kind::System, format!("Error: {error:#}"))),
    }
    app.status = status_line(store, registry, config);
    app.title = title_line(store, config);
    refresh_tabs(app, store, config);
    app.follow = true;
}

/// Suspend the TUI, run a login flow (browser/device prompts visible), wait for
/// the user, then resume the TUI.
fn run_login(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    store: &mut SessionStore,
    registry: &Registry,
    config: &AppConfig,
    command: &str,
) {
    use std::io::Write;
    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    println!("\nConnecting… follow any browser window or device-code prompt below.\n");
    let _ = io::stdout().flush();
    match handle_input(store, registry, config, command) {
        Ok(output) => {
            println!("{output}");
            // Switch to the logged-in provider's model so the user doesn't have
            // to hunt for it (fixes "logged in but it's looking for a key").
            if let Some(provider) = command
                .strip_prefix("/login ")
                .and_then(|rest| rest.split_whitespace().next())
                && let Ok(switched) =
                    handle_input(store, registry, config, &format!("/model {provider}"))
            {
                println!("→ {switched}");
            }
        }
        Err(error) => println!("Login failed: {error:#}"),
    }
    println!("\nPress Enter to return to bbarit.");
    let _ = io::stdout().flush();
    // Wait via crossterm key events, not read_line: xterm.js sends a bare CR for
    // Enter and Windows ConPTY doesn't restore cooked line input after
    // disable_raw_mode, so read_line (which needs an LF) would block forever
    // (terminal-win#6). Re-enable raw mode first so crossterm reads keys directly.
    enable_raw_mode().ok();
    wait_for_login_ack();
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture).ok();
    terminal.clear().ok();
}

/// A key press that dismisses the post-login "Press Enter to return" prompt.
/// Enter is the advertised key; Esc and Ctrl+C also return so the user is never
/// stranded on the login screen.
fn is_login_ack_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Esc)
        || matches!(
            (code, modifiers),
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
        )
}

/// Block until the user acknowledges the login result. Reads crossterm key
/// events (raw mode) instead of read_line so it works regardless of the
/// terminal's CR/LF line discipline (terminal-win#6).
fn wait_for_login_ack() {
    loop {
        match event::read() {
            Ok(Event::Key(key))
                if key.kind == KeyEventKind::Press && is_login_ack_key(key.code, key.modifiers) =>
            {
                return;
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Run the given input, then drain any messages queued (typed) while it ran.
fn run_turn(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut SessionStore,
    registry: &Registry,
    config: &mut AppConfig,
    input: &str,
) -> io::Result<()> {
    run_single_turn(terminal, app, store, registry, config, input)?;
    while !app.queued.is_empty() && !app.exit {
        // Cancelling does not discard queued input — it stays in the panel and
        // runs after the next submitted turn finishes. To clear it, drop it explicitly
        // with Esc (empty input box) while idle.
        if crate::commands::cancel_requested() {
            app.status = format!(
                "cancelled — {} queued message(s) kept · Esc discards them",
                app.queued.len()
            );
            break;
        }
        // Queued slash commands go back through dispatch_line (TUI-only
        // commands like /worktree would otherwise reach the model as an
        // unknown command); consecutive plain messages still batch into one
        // turn so follow-ups typed mid-turn don't each pay a slow turn.
        let next = app.queued.remove(0);
        if next.starts_with('/') {
            dispatch_line(terminal, app, store, registry, config, &next)?;
        } else {
            let mut batch = vec![next];
            while app
                .queued
                .first()
                .is_some_and(|line| !line.starts_with('/'))
            {
                batch.push(app.queued.remove(0));
            }
            run_single_turn(terminal, app, store, registry, config, &batch.join("\n\n"))?;
        }
    }
    refresh_tabs(app, store, config);
    Ok(())
}

/// If the clipboard holds an image, save it as a temp PNG and return an
/// `@<path>` token (handle_input attaches `@*.png` as vision input on send).
fn clipboard_image_token() -> Option<String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let image = clipboard.get_image().ok()?;
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, image.width as u32, image.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&image.bytes).ok()?;
    }
    let index = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("bbarit-paste-{}-{index}.png", std::process::id()));
    std::fs::write(&path, &buf).ok()?;
    Some(format!("@{}", path.display()))
}

/// A Knight-Rider style bouncing block, animated by the render tick.
fn working_bar(tick: usize) -> String {
    const WIDTH: usize = 10;
    let period = WIDTH * 2 - 2;
    let raw = tick % period;
    let pos = if raw < WIDTH { raw } else { period - raw };
    (0..WIDTH)
        .map(|index| if index == pos { '▰' } else { '▱' })
        .collect()
}

/// Run one turn on a worker thread, streaming deltas into a partial assistant
/// entry while the UI keeps rendering and accepts input (Esc cancels, Enter
/// queues a follow-up). Turn errors become a System entry, not a crash.
fn run_single_turn(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut SessionStore,
    registry: &Registry,
    config: &AppConfig,
    input: &str,
) -> io::Result<()> {
    crate::commands::reset_cancel();
    app.entries.push(Entry::new(Kind::User, input.to_string()));
    app.follow = true;
    let before = store.messages().len();

    let (tx, rx) = mpsc::channel::<String>();
    const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut partial = String::new();
    let mut tick = 0usize;
    let mut cancelling = false;
    let started = std::time::Instant::now();
    // Badge for the started mode (harness / multi-job / …), shown in the bar.
    let mode_badge = turn_mode_label(input)
        .map(|mode| format!(" ⟪{mode}⟫"))
        .unwrap_or_default();
    app.working = true;
    let worker_store: &mut SessionStore = &mut *store;
    let outcome: Result<String> = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            // STREAM_SINK is thread-local; install it on THIS worker thread so
            // streamed tokens and live tool activity actually reach the UI.
            crate::llm::set_stream_sink(Some(Box::new(move |chunk: &str| {
                let _ = tx.send(chunk.to_string());
            })));
            let result = handle_input(worker_store, registry, config, input);
            crate::llm::set_stream_sink(None);
            result
        });
        loop {
            while let Ok(chunk) = rx.try_recv() {
                partial.push_str(&chunk);
                // Don't force follow here: if the user scrolled up to read
                // earlier output while the agent streams, stay where they are
                // (follow resumes when they scroll back to the bottom).
            }
            // Always-visible animated working indicator in the top bar, with the
            // current activity (last ⚙/✓/✗ line) so you always see what it's doing.
            let spin = SPINNER[tick % SPINNER.len()];
            let bar = working_bar(tick);
            let dots = ".".repeat(1 + (tick / 2) % 3);
            let activity = partial
                .lines()
                .rev()
                .find(|line| {
                    let t = line.trim_start();
                    t.starts_with('⚙') || t.starts_with('✓') || t.starts_with('✗')
                })
                .map(|line| {
                    let line = line.trim();
                    let preview: String = line.chars().take(48).collect();
                    format!("  ·  {preview}")
                })
                .unwrap_or_default();
            app.working_text = if cancelling {
                format!(" ✗ cancelling… stops after the current step  {bar}")
            } else {
                format!(
                    " {spin} working{mode_badge}{dots}  {bar}  {}s{activity}   (Esc to cancel)",
                    started.elapsed().as_secs()
                )
            };
            // The bottom working bar already shows the animated "working" state
            // (with elapsed time and Esc hint), so DON'T also inject a spinner
            // placeholder into the transcript — boxed as a bbarit card it looked
            // like a real message and read as a confusing second "working".
            // Render a transcript card only once real streamed content exists.
            let render_partial = if partial.is_empty() {
                None
            } else {
                Some(partial.as_str())
            };
            let _ = terminal.draw(|frame| render(frame, app, render_partial));
            tick += 1;
            if worker.is_finished() {
                while let Ok(chunk) = rx.try_recv() {
                    partial.push_str(&chunk);
                }
                return worker
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("turn thread panicked")));
            }
            // Accept input while the turn runs: Esc cancels (after the current
            // call), Enter queues a follow-up, paste/keys edit the input line.
            {
                // Same drain-before-redraw as the idle loop: one full-frame
                // redraw per character made typing crawl while a turn streamed.
                // next_events also reassembles bracketed pastes, so pasting
                // mid-turn can no longer read as Esc (= cancel).
                for event in next_events(Duration::from_millis(80)).unwrap_or_default() {
                    match event {
                        Event::Paste(text) => app.input.push_str(&text),
                        // Wheel scroll + drag-select/copy keep working while streaming.
                        Event::Mouse(mouse) => {
                            let render_partial = if partial.is_empty() {
                                None
                            } else {
                                Some(partial.as_str())
                            };
                            handle_transcript_mouse(app, mouse, render_partial);
                        }
                        Event::Key(key) if key.kind == KeyEventKind::Press => {
                            match (key.code, key.modifiers) {
                                (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                                    crate::commands::request_cancel();
                                    cancelling = true;
                                }
                                (KeyCode::PageUp, _) => app.scroll_up_by(10),
                                (KeyCode::PageDown, _) => app.scroll_down_by(10),
                                (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                                    app.input.clear();
                                }
                                (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                                    app.entries.clear();
                                    partial.clear();
                                    app.follow = true;
                                    app.scroll = 0;
                                    terminal.clear().ok();
                                }
                                (KeyCode::Enter, m)
                                    if m.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
                                {
                                    app.input.push('\n');
                                }
                                (KeyCode::Enter, _) => {
                                    let text = app.input.trim().to_string();
                                    app.input.clear();
                                    if matches!(text.as_str(), "/clear" | "/cls") {
                                        app.entries.clear();
                                        partial.clear();
                                        app.follow = true;
                                        app.scroll = 0;
                                        terminal.clear().ok();
                                    } else if !text.is_empty() {
                                        // Input queued mid-turn is also kept in Up/Down history.
                                        app.record_history(config, &text);
                                        app.queued.push(text);
                                    }
                                }
                                (KeyCode::Backspace, _) => {
                                    app.input.pop();
                                }
                                // Recall previous inputs with Up/Down even mid-turn —
                                // since the agent running is effectively most of the time,
                                // having it only in the idle composer would make it feel broken.
                                (KeyCode::Up, _) => app.history_prev(),
                                (KeyCode::Down, _) => app.history_next(),
                                // Don't let an unhandled Ctrl/Alt chord (e.g.
                                // Alt+1..9, Ctrl+W) type its raw letter mid-turn.
                                (KeyCode::Char(ch), m)
                                    if !m.intersects(
                                        KeyModifiers::CONTROL
                                            | KeyModifiers::ALT
                                            | KeyModifiers::SUPER,
                                    ) =>
                                {
                                    app.input.push(ch);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    match outcome {
        Ok(output) => {
            let messages = store.messages();
            if messages.len() > before {
                for message in &messages[before..] {
                    let kind = match message.role {
                        Role::User => continue,
                        Role::Assistant => Kind::Assistant,
                        Role::Tool => Kind::Tool,
                    };
                    app.entries.push(Entry::new(kind, message.content.clone()));
                }
            } else if !output.trim().is_empty() {
                app.entries.push(Entry::new(Kind::System, output));
            }
        }
        Err(error) => {
            let message = format!("{error:#}");
            // A "no API key" turn is a dead end — turn it into an actionable login
            // flow: name the provider and open the picker, so the user is one
            // keystroke from signing in instead of re-reading an error.
            let provider = message
                .split_once("No API key for ")
                .map(|(_, rest)| rest)
                .and_then(|rest| rest.split(|c: char| c == '.' || c.is_whitespace()).next())
                .filter(|p| !p.is_empty())
                .map(str::to_string);
            if let Some(provider) = provider {
                if provider_supports_browser_login(&provider) {
                    app.entries.push(Entry::new(
                        Kind::System,
                        format!(
                            "🔑 Not signed in to {provider}. Pick how to sign in below — or \
                             press Esc and switch to a model you have a key for with /model."
                        ),
                    ));
                    app.selector = Some(login_selector(registry));
                } else {
                    // Key-only provider: the model already named it, so skip the
                    // provider picker and open the masked key prompt directly —
                    // one paste-and-Enter connects.
                    app.entries.push(Entry::new(
                        Kind::System,
                        format!(
                            "🔑 Not signed in to {provider}. Paste your API key to connect — or \
                             press Esc and switch to a model you have a key for with /model."
                        ),
                    ));
                    app.prompt = Some(Prompt::for_command(&format!("/login {provider} ")));
                }
            } else {
                app.entries
                    .push(Entry::new(Kind::System, format!("Error: {message}")));
            }
            // The screen may be polluted on the error path (external output / partial-render residue),
            // so fully repaint the next frame so the bottom UI (input box / status line) is always visible.
            terminal.clear().ok();
        }
    }
    app.working = false;
    app.status = status_line(store, registry, config);
    app.title = title_line(store, config);
    app.follow = true;
    Ok(())
}

fn render(frame: &mut Frame, app: &mut App, partial: Option<&str>) {
    let area = frame.area();

    // When the selector overlay is open it owns the lower panes (no suggestions).
    let suggestions = if app.selector.is_none() {
        command_suggestions(&app.input)
    } else {
        Vec::new()
    };
    let suggest_h = (suggestions.len() as u16).min(6);

    // Input box grows with multiline content (Alt/Shift+Enter, multi-line paste),
    // up to 8 rows + border. Long lines wrap, so count VISUAL rows, not '\n's.
    let input_inner_w = area.width.saturating_sub(2).max(1) as usize;
    let input_lines: u16 = app
        .input
        .split('\n')
        .map(|l| (UnicodeWidthStr::width(l) / input_inner_w) as u16 + 1)
        .sum::<u16>()
        .max(1);
    let input_h = (input_lines + 2).min(10);

    // Messages typed while the agent is working wait in a queue; show them so
    // the user sees their input registered (they run after the current turn).
    let queued_h = if app.queued.is_empty() {
        0
    } else {
        (app.queued.len() as u16 + 1).min(6)
    };

    // The "working" bar now sits at the BOTTOM, just above the input, so
    // it's where your eyes already are while typing.
    let working_h = if app.working { 1 } else { 0 };

    // External processes this agent launched (bash background:true / auto-bg):
    // a one-line footer so they're always visible while running.
    let bg_jobs = crate::tools::background_job_summaries();
    let running_jobs: Vec<(usize, u32, String)> = bg_jobs
        .into_iter()
        .filter(|(_, _, running, _)| *running)
        .map(|(id, pid, _, cmd)| (id, pid, cmd))
        .collect();
    let procs_h = if running_jobs.is_empty() { 0 } else { 1 };

    let mut constraints = vec![Constraint::Length(1), Constraint::Min(1)];
    if queued_h > 0 {
        constraints.push(Constraint::Length(queued_h));
    }
    if suggest_h > 0 {
        constraints.push(Constraint::Length(suggest_h));
    }
    if working_h > 0 {
        constraints.push(Constraint::Length(working_h));
    }
    if procs_h > 0 {
        constraints.push(Constraint::Length(procs_h));
    }
    constraints.push(Constraint::Length(input_h));
    constraints.push(Constraint::Length(1)); // command hints
    constraints.push(Constraint::Length(1)); // status footer
    let chunks = Layout::vertical(constraints).split(area);
    let mut index = 2usize;
    let queued_chunk = if queued_h > 0 {
        let chunk = chunks[index];
        index += 1;
        Some(chunk)
    } else {
        None
    };
    let suggest_chunk = if suggest_h > 0 {
        let chunk = chunks[index];
        index += 1;
        Some(chunk)
    } else {
        None
    };
    let working_chunk = if working_h > 0 {
        let chunk = chunks[index];
        index += 1;
        Some(chunk)
    } else {
        None
    };
    let procs_chunk = if procs_h > 0 {
        let chunk = chunks[index];
        index += 1;
        Some(chunk)
    } else {
        None
    };
    let input_chunk = chunks[index];
    let hints_chunk = chunks[index + 1];
    let menu_chunk = chunks[index + 2];

    // Top bar: working indicator during a turn, else the current session title.
    //
    // A horizontal session-tab strip used to live here, but with many sessions it
    // overflowed the single-row top bar and broke the layout. Session selection is
    // now the vertical `/sessions` picker (Alt+N starts a new one); the top bar
    // just names the active session so it never wraps.
    // Working bar (if any) is drawn at the bottom near the input — see working_chunk.
    if let Some(working_chunk) = working_chunk {
        // Working bar (D4): a quiet accent line instead of a full reversed bar
        // — the animation carries the "alive" signal; a solid background field
        // right above the input was shouting. Cancelling turns the line red;
        // the trailing "(Esc …)" hint recedes to the frame gray.
        let cancelling = app.working_text.trim_start().starts_with('✗');
        let tone = if cancelling { BRAND_RED } else { ACCENT };
        let text = app.working_text.clone();
        let (main, hint) = match text.split_once("(Esc") {
            Some((main, rest)) => (main.to_string(), format!("(Esc{rest}")),
            None => (text, String::new()),
        };
        let mut spans = vec![
            Span::styled("▍", Style::new().fg(tone)),
            Span::styled(main, Style::new().fg(tone).add_modifier(Modifier::BOLD)),
        ];
        if !hint.is_empty() {
            spans.push(Span::styled(hint, Style::new().fg(CARD_FRAME)));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), working_chunk);
    }
    if let Some(procs_chunk) = procs_chunk {
        // External-process footer: pid + command per running background job,
        // so what this agent launched is always visible.
        let mut spans = vec![Span::styled(
            format!("⚡ processes {}  ", running_jobs.len()),
            Style::new().fg(AGENT_GREEN).add_modifier(Modifier::BOLD),
        )];
        for (i, (id, pid, cmd)) in running_jobs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    "  ·  ".to_string(),
                    Style::new().fg(CARD_FRAME),
                ));
            }
            let short: String = cmd.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
            let short: String = short.chars().take(40).collect();
            spans.push(Span::styled(
                format!("#{id} pid {pid}"),
                Style::new().fg(Color::Reset),
            ));
            if !short.is_empty() {
                spans.push(Span::styled(
                    format!(" {short}"),
                    Style::new().fg(CARD_FRAME),
                ));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), procs_chunk);
    }
    {
        // Top bar (D2): centered session title with the standing-mode badges
        // restyled as gold chips, plus a brand chip pinned on the left. The
        // chip renders AFTER the title so it wins if a long title overlaps.
        let (base, badges) = match app.title.split_once("   ⟪") {
            Some((base, rest)) => (
                base.to_string(),
                Some(rest.trim_end_matches('⟫').to_string()),
            ),
            None => (app.title.clone(), None),
        };
        let mut title_spans = vec![Span::styled(
            base,
            Style::new().add_modifier(Modifier::BOLD),
        )];
        if let Some(badges) = badges {
            title_spans.push(Span::raw("  "));
            for (i, badge) in badges.split(" · ").enumerate() {
                if i > 0 {
                    title_spans.push(Span::styled(" ", Style::new()));
                }
                title_spans.push(Span::styled(
                    format!("⟪{badge}⟫"),
                    Style::new().fg(GOLD).add_modifier(Modifier::BOLD),
                ));
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(title_spans)).alignment(ratatui::layout::Alignment::Center),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " ● ",
                    Style::new().fg(BRAND_RED).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "BBARIT",
                    Style::new().fg(BRAND_RED).add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[0],
        );
    }

    // Single-column layout (portable / easy to read): the chat uses the full
    // width. The plan, wiki and projects are reached inline (todo output, /wiki,
    // /cd) rather than as side panels.
    let body = chunks[1];
    let transcript = body;
    // Cards are laid out (wrapped + boxed + folded) at the real transcript
    // width, so each cached line already fits one row: the scroll total is
    // just the row-count sum. Pass 1 walks COUNTS only (cached per entry) and
    // pass 2 materializes just the visible window — the per-frame cost stays
    // constant no matter how long the session transcript grows.
    let width = transcript.width.max(1) as usize;
    let visible_rows = transcript.height as usize;
    enum TranscriptBlock<'a> {
        Divider(usize),
        Entry(&'a Entry),
        Partial,
    }
    let live_lines = match partial {
        Some(text) if !text.is_empty() => Some(live_partial_lines(text, width, app.tools_expanded)),
        _ => None,
    };
    let mut blocks: Vec<(TranscriptBlock, usize)> = Vec::with_capacity(app.entries.len() * 2 + 1);
    let mut task_no = 0usize;
    for entry in &app.entries {
        // Task (turn) separation: before each user instruction insert a "── task N ──" divider
        // to visually group one instruction with its tools/responses.
        if entry.kind == Kind::User {
            task_no += 1;
            blocks.push((TranscriptBlock::Divider(task_no), 1));
        }
        blocks.push((
            TranscriptBlock::Entry(entry),
            entry_line_count(entry, width, app.tools_expanded),
        ));
    }
    if let Some(live) = &live_lines {
        blocks.push((TranscriptBlock::Partial, live.len()));
    }
    let content_total: usize = blocks.iter().map(|(_, count)| count).sum();
    // Bottom-align a short transcript: pad rows occupy the top of the
    // absolute row space, exactly as the old full-materialization did.
    let pad = if app.follow && visible_rows > 0 && content_total < visible_rows {
        visible_rows - content_total
    } else {
        0
    };
    let total = content_total + pad;
    let max_scroll = total.saturating_sub(visible_rows) as u16;
    // Anchor for scroll_up_by/scroll_down_by: where "the bottom" currently is.
    app.last_max_scroll = max_scroll;
    let scroll = if app.follow {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    // Record the frame geometry for mouse row mapping (drag-select).
    app.last_transcript_area = transcript;
    app.last_scroll = scroll;
    // Pass 2 — materialize rows [start, end) only.
    let start = scroll as usize;
    let end = (start + visible_rows).min(total);
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(end.saturating_sub(start));
    for _ in start..pad.min(end) {
        lines.push(Line::raw(""));
    }
    let mut offset = pad;
    for (block, count) in &blocks {
        if offset >= end {
            break;
        }
        let block_start = offset;
        offset += count;
        if offset <= start {
            continue;
        }
        let lo = start.saturating_sub(block_start);
        let hi = (end - block_start).min(*count);
        match block {
            TranscriptBlock::Divider(n) => lines.push(turn_divider(*n, width)),
            TranscriptBlock::Entry(entry) => {
                let cached = entry_lines(entry, width, app.tools_expanded);
                lines.extend(cached[lo..hi].iter().cloned());
            }
            TranscriptBlock::Partial => {
                if let Some(live) = &live_lines {
                    lines.extend(live[lo..hi].iter().cloned());
                }
            }
        }
    }
    // Highlight the selected rows (absolute indices) so the user sees what
    // release will copy.
    if let Some((a, b)) = app.mouse_sel {
        let (lo, hi) = (a.min(b), a.max(b));
        for (i, line) in lines.iter_mut().enumerate() {
            let absolute = start + i;
            if absolute >= lo && absolute <= hi {
                line.style = line.style.add_modifier(Modifier::REVERSED);
            }
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), transcript);
    if let Some(area) = queued_chunk {
        let lines: Vec<Line> = app
            .queued
            .iter()
            .map(|message| {
                let preview: String = message.replace('\n', " ").chars().take(80).collect();
                Line::from(vec![
                    Span::styled(" ⏳ ".to_string(), Style::new().fg(GOLD)),
                    Span::styled(preview, Style::new().fg(Color::Gray)),
                ])
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(format!(
                        " {} queued — runs after the current task · Esc(idle, empty input) discards ",
                        app.queued.len()
                    ))
                    .title_style(Style::new().fg(GOLD).add_modifier(Modifier::BOLD))
                    .border_style(Style::new().fg(CARD_FRAME)),
            ),
            area,
        );
    }

    if let Some(area) = suggest_chunk {
        let mut lines = Vec::new();
        for (index, command) in suggestions.iter().enumerate() {
            // Tab-completion target (first row) gets the fixed selection pair;
            // the alternatives sit quietly in the accent color.
            let (marker, style) = if index == 0 {
                (
                    " ❯ ",
                    Style::new()
                        .fg(SEL_FG)
                        .bg(SEL_BG)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("   ", Style::new().fg(ACCENT))
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{command} "),
                style,
            )));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    let input = input_chunk;
    // Input box (D3): rounded, accent-bordered — THE primary element on the
    // screen. While a turn runs it recedes to the frame gray (typing queues).
    // An empty box shows a dim placeholder instead of sitting blank.
    let title = if suggest_h > 0 {
        " Tab complete · Enter send "
    } else {
        " Enter send · Alt+Enter newline "
    };
    let border_tone = if app.working { CARD_FRAME } else { ACCENT };
    // When the input outgrows the box, scroll so the end (where typing happens)
    // stays visible.
    let input_visible_rows = input_h.saturating_sub(2).max(1);
    let input_scroll = input_lines.saturating_sub(input_visible_rows);
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border_tone))
        .title(title)
        .title_style(Style::new().fg(CARD_FRAME));
    let input_paragraph = if app.input.is_empty() {
        Paragraph::new(Span::styled(
            "Ask anything — /help for commands",
            Style::new().fg(CARD_FRAME).add_modifier(Modifier::ITALIC),
        ))
    } else {
        Paragraph::new(app.input.as_str())
    };
    frame.render_widget(
        input_paragraph
            .wrap(Wrap { trim: false })
            .scroll((input_scroll, 0))
            .block(input_block),
        input,
    );
    // Cursor sits at the end of the last VISUAL row (wrapped rows counted).
    let last_line = app.input.split('\n').next_back().unwrap_or("");
    let row = input_lines.saturating_sub(1).saturating_sub(input_scroll);
    let cursor_x = (input.x + 1 + (UnicodeWidthStr::width(last_line) % input_inner_w) as u16)
        .min(input.x + input.width.saturating_sub(2));
    frame.set_cursor_position(Position::new(
        cursor_x.min(area.right().saturating_sub(1)),
        (input.y + 1 + row).min(area.bottom().saturating_sub(1)),
    ));

    // Command hints line (always visible) above the powerline footer.
    // Keys pop in the accent; labels recede so the row scans as a keymap, not
    // a sentence.
    let hint = |key: &'static str, label: &'static str| {
        vec![
            Span::styled(key, Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {label}"), Style::new().fg(CARD_FRAME)),
            Span::styled("  ", Style::new()),
        ]
    };
    let mut hint_spans = vec![Span::raw(" ")];
    // Esc cancels the running turn; when idle it clears the composer instead.
    let esc_label = if app.working { "cancel" } else { "clear" };
    for (key, label) in [
        ("Tab", "menu"),
        ("/model", ""),
        ("/harness", ""),
        ("/files", ""),
        ("/resume", ""),
        ("/goal", ""),
        ("^T", "fold"),
        ("Esc", esc_label),
        ("/help", ""),
    ] {
        hint_spans.extend(hint(key, label));
    }
    frame.render_widget(Paragraph::new(Line::from(hint_spans)), hints_chunk);

    // Bottom: powerline footer (D6). The status string is segmented
    // on "  ·  ": project/branch leads in the accent, token counters recede,
    // the model (last segment) stays bold, and a PLAN segment warns in gold.
    {
        // On narrow terminals Paragraph clips the right edge — exactly where
        // the model lives — so it reads as "the model name never shows".
        // Shed detail in reverse importance (account email, then the branch,
        // then the model's provider prefix) until the line fits.
        let mut segments: Vec<String> = app.status.split("  ·  ").map(|s| s.to_string()).collect();
        let width = menu_chunk.width as usize;
        let too_wide = |segs: &[String]| {
            segs.iter().map(|s| s.chars().count()).sum::<usize>() + segs.len().saturating_sub(1) * 5
                > width
        };
        if too_wide(&segments) && segments.len() > 1 {
            segments.retain(|s| !s.contains('@'));
        }
        if too_wide(&segments) {
            let first = usize::from(segments.first().is_some_and(|s| s.contains("PLAN")));
            if let Some(project) = segments.get_mut(first) {
                if let Some(idx) = project.find(" (") {
                    project.truncate(idx);
                }
            }
        }
        if too_wide(&segments) {
            if let Some(model) = segments.last_mut() {
                if let Some(idx) = model.rfind('/') {
                    *model = model[idx + 1..].to_string();
                }
            }
        }
        let last = segments.len().saturating_sub(1);
        let mut spans: Vec<Span> = Vec::new();
        for (i, segment) in segments.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  ·  ", Style::new().fg(CARD_FRAME)));
            }
            // PLAN mode ships as its own leading segment, so "first" here means
            // "first non-PLAN segment" — that's the project/branch.
            let style = if segment.contains("PLAN") {
                Style::new().fg(GOLD).add_modifier(Modifier::BOLD)
            } else if i == 0 || (i == 1 && segments[0].contains("PLAN")) {
                Style::new().fg(ACCENT)
            } else if i == last {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Gray)
            };
            spans.push(Span::styled(segment.to_string(), style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), menu_chunk);
    }

    // The menu/selector floats as a centered popup over the chat (which stays
    // visible behind it) instead of replacing the whole screen.
    if let Some(selector) = &app.selector {
        render_selector_popup(frame, selector, body);
    }
    if let Some(prompt) = &app.prompt {
        render_prompt_popup(frame, prompt, body);
    }

    // On terminals without truecolor (e.g. Terminal.app), Rgb is ignored wholesale and the screen
    // looks all default (white). At the end of render, sweep the whole buffer once and drop Rgb
    // to 256-color indices — covering syntax highlighting too without touching 63 color constants.
    if !crate::themes::truecolor_supported() {
        for cell in frame.buffer_mut().content.iter_mut() {
            if let Color::Rgb(r, g, b) = cell.fg {
                cell.fg = Color::Indexed(crate::themes::rgb_to_indexed(r, g, b));
            }
            if let Color::Rgb(r, g, b) = cell.bg {
                cell.bg = Color::Indexed(crate::themes::rgb_to_indexed(r, g, b));
            }
        }
    }
}

/// Render the in-menu input overlay: a centered box with a titled single-line
/// field (masked for secrets) so a value is typed inside the menu.
fn render_prompt_popup(frame: &mut Frame, prompt: &Prompt, body: ratatui::layout::Rect) {
    let width = (body.width * 3 / 4).clamp(40, 88).min(body.width);
    let height = 5u16.min(body.height);
    let area = ratatui::layout::Rect {
        x: body.x + body.width.saturating_sub(width) / 2,
        y: body.y + body.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(ratatui::widgets::Clear, area);
    // Title rides the border as a solid accent chip (fixed fg/bg pair) so the
    // popup reads as one designed component on any terminal theme.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {} ", prompt.title))
        .title_style(
            Style::new()
                .fg(SEL_FG)
                .bg(SEL_BG)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::new().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    let (field_area, _spacer, hint_area) = (rows[0], rows[1], rows[2]);
    let shown = if prompt.masked {
        "•".repeat(prompt.value.chars().count())
    } else {
        prompt.value.clone()
    };
    let field = format!("❯ {shown}");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(shown.clone(), Style::new().fg(Color::Reset)),
        ])),
        field_area,
    );
    frame.render_widget(
        Paragraph::new(" Enter confirm · Esc cancel · paste supported")
            .style(Style::new().fg(CARD_FRAME)),
        hint_area,
    );
    let cursor_x = (field_area.x + UnicodeWidthStr::width(field.as_str()) as u16)
        .min(field_area.x + field_area.width.saturating_sub(1));
    frame.set_cursor_position(Position::new(cursor_x, field_area.y));
}

const SLASH_COMMANDS: &[&str] = &[
    "/model",
    "/ollama",
    "/login",
    "/logout",
    "/computer",
    "/accounts",
    "/settings",
    "/sessions",
    "/new",
    "/clone",
    "/fork",
    "/resume",
    "/name",
    "/review",
    "/bugfix",
    "/batch",
    "/loop",
    "/goal",
    "/improve",
    "/autoimprove",
    "/harness",
    "/roles",
    "/persona",
    "/orchestrate",
    "/lens",
    "/restore",
    "/bench",
    "/land",
    "/files",
    "/cd",
    "/context",
    "/deps",
    "/plan",
    "/summarize",
    "/wiki",
    "/compact",
    "/tree",
    "/label",
    "/branch",
    "/worktree",
    "/read",
    "/write",
    "/edit",
    "/ls",
    "/find",
    "/grep",
    "/bash",
    "/mcp",
    "/interop",
    "/update",
    "/hooks",
    "/thinking",
    "/theme",
    "/themes",
    "/prompt",
    "/prompts",
    "/skill",
    "/skills",
    "/export",
    "/import",
    "/share",
    "/memory",
    "/session",
    "/history",
    "/trust",
    "/links",
    "/menu",
    "/help",
    "/exit",
    "/quit",
];

/// Slash-command completions for the current input (only while typing the
/// command word, before any space).
fn command_suggestions(input: &str) -> Vec<String> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') || trimmed.contains(' ') || trimmed.len() < 2 {
        return Vec::new();
    }
    let needle = trimmed.to_lowercase();
    SLASH_COMMANDS
        .iter()
        .filter(|command| command.starts_with(&needle) && **command != needle)
        .map(|command| command.to_string())
        .collect()
}

/// Render the selector as a centered popup over `body` (chat stays visible).
fn render_selector_popup(frame: &mut Frame, selector: &Selector, body: ratatui::layout::Rect) {
    let items = selector.filtered_indexed();
    // Gauge rows (accounts dashboard) need the full row to fit — widen the
    // popup when any styled row is present.
    let max_width = if selector.styled.is_empty() { 88 } else { 110 };
    let width = (body.width * 3 / 4).clamp(44, max_width).min(body.width);
    let max_rows = body.height.saturating_sub(2).max(6);
    let height = ((items.len() as u16) + 6)
        .clamp(6, max_rows)
        .min(body.height);
    let area = ratatui::layout::Rect {
        x: body.x + body.width.saturating_sub(width) / 2,
        y: body.y + body.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(ratatui::widgets::Clear, area);
    // Same chip-on-border treatment as the prompt popup — one component
    // language across every overlay.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {} ", selector.title))
        .title_style(
            Style::new()
                .fg(SEL_FG)
                .bg(SEL_BG)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::new().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let (list_area, filter_area, hint_area) = (rows[0], rows[1], rows[2]);

    let visible = list_area.height as usize;
    let start = if visible > 0 && selector.cursor >= visible {
        selector.cursor + 1 - visible
    } else {
        0
    };
    let mut lines = Vec::new();
    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no matches)",
            Style::new().fg(Color::Reset),
        )));
    }
    for (index, entry) in items.iter().enumerate().skip(start).take(visible) {
        let item_index = entry.0;
        let (label, command) = entry.1;
        if command.is_empty() {
            // Group header: a dim gold section label, not a selectable row.
            lines.push(Line::from(Span::styled(
                format!("  {}", label.to_ascii_uppercase()),
                Style::new().fg(GOLD).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        let selected = index == selector.cursor;
        // Rich row (accounts dashboard): keep its cell colors; selection is
        // shown by the marker + highlighting the first (label) span.
        if let Some(styled) = selector.styled.get(&item_index) {
            let mut line = styled.clone();
            let mut spans = vec![if selected {
                Span::styled(
                    "❯ ",
                    Style::new()
                        .fg(SEL_FG)
                        .bg(SEL_BG)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            }];
            if selected && let Some(first) = line.spans.first_mut() {
                first.style = Style::new()
                    .fg(SEL_FG)
                    .bg(SEL_BG)
                    .add_modifier(Modifier::BOLD);
            }
            spans.extend(line.spans);
            lines.push(Line::from(spans));
            continue;
        }
        let (prefix, style) = if selected {
            // Fixed fg/bg selection pair — contrast never depends on the theme.
            (
                "❯ ",
                Style::new()
                    .fg(SEL_FG)
                    .bg(SEL_BG)
                    .add_modifier(Modifier::BOLD),
            )
        } else if matches!(
            command.as_str(),
            "@menu" | "@close" | "@back:providers" | "@back:roles"
        ) {
            ("  ", Style::new().fg(Color::Gray))
        } else {
            // Color::Reset = the terminal's own default foreground, so list items
            // stay visible on both light and dark themes (White vanished on light).
            ("  ", Style::new().fg(Color::Reset))
        };
        lines.push(Line::from(Span::styled(format!("{prefix}{label}"), style)));
    }
    frame.render_widget(Paragraph::new(lines), list_area);
    let filter_text = format!(" ❯ {}", selector.filter);
    frame.render_widget(
        Paragraph::new(filter_text.clone())
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        filter_area,
    );
    frame.render_widget(
        Paragraph::new(" ↑↓ move · Enter select · Esc close · type to search")
            .style(Style::new().fg(CARD_FRAME)),
        hint_area,
    );
    let cursor_x = (filter_area.x + UnicodeWidthStr::width(filter_text.as_str()) as u16)
        .min(filter_area.x + filter_area.width.saturating_sub(1));
    frame.set_cursor_position(Position::new(cursor_x, filter_area.y));
}

#[allow(dead_code)]
fn render_selector(
    frame: &mut Frame,
    selector: &Selector,
    list_area: ratatui::layout::Rect,
    filter_area: ratatui::layout::Rect,
    footer_area: ratatui::layout::Rect,
) {
    let items = selector.filtered();
    let height = list_area.height as usize;
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(" {}", selector.title),
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    let visible = height.saturating_sub(1);
    let start = if visible > 0 && selector.cursor >= visible {
        selector.cursor + 1 - visible
    } else {
        0
    };
    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no matches)",
            Style::new().fg(Color::Reset),
        )));
    }
    for (index, (label, command)) in items.iter().enumerate().skip(start).take(visible) {
        if command.is_empty() {
            // Brand/group header (not selectable).
            lines.push(Line::from(Span::styled(
                label.clone(),
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        let style = if index == selector.cursor {
            Style::new().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::new()
        };
        let marker = if index == selector.cursor { "> " } else { "  " };
        lines.push(Line::from(Span::styled(format!("{marker}{label}"), style)));
    }
    frame.render_widget(Paragraph::new(lines), list_area);

    let filter_text = format!("filter: {}", selector.filter);
    frame.render_widget(
        Paragraph::new(filter_text.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" type to filter "),
        ),
        filter_area,
    );
    let frame_area = frame.area();
    let cursor_x = (filter_area.x + 1 + UnicodeWidthStr::width(filter_text.as_str()) as u16)
        .min(filter_area.x + filter_area.width.saturating_sub(2));
    frame.set_cursor_position(Position::new(
        cursor_x.min(frame_area.right().saturating_sub(1)),
        (filter_area.y + 1).min(frame_area.bottom().saturating_sub(1)),
    ));

    frame.render_widget(
        Paragraph::new(" ↑/↓ move · Enter select · Esc cancel · type to filter")
            .style(Style::new().fg(Color::Reset)),
        footer_area,
    );
}

fn seed_entries(store: &SessionStore) -> Vec<Entry> {
    store
        .messages()
        .iter()
        .map(|message| {
            Entry::new(
                match message.role {
                    Role::User => Kind::User,
                    Role::Assistant => Kind::Assistant,
                    Role::Tool => Kind::Tool,
                },
                message.content.clone(),
            )
        })
        .collect()
}

fn status_line(store: &SessionStore, registry: &Registry, config: &AppConfig) -> String {
    let project = config
        .cwd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| config.cwd.display().to_string());
    let branch = git_branch(&config.cwd)
        .map(|branch| format!(" ({branch})"))
        .unwrap_or_default();
    // Show the model that the next turn will use (current session model, else the
    // configured default) so the name appears from the first frame — not only
    // after the first run.
    let model = crate::commands::default_model_label(store, registry, config)
        .unwrap_or_else(|| "no model".to_string());
    let usage = store.token_usage_total();

    // powerline: project (branch) · ↑in ↓out Rcached $cost · ctx%/window · model
    let mut context = String::new();
    if let Some((percent, window)) = context_usage(store, registry) {
        context = format!(" · {percent}%/{}k", window / 1000);
    }
    // Read-only Plan Mode marker (set via /plan), so it's obvious edits are paused.
    let plan = if crate::commands::plan_mode_active() {
        "⏸ PLAN  ·  "
    } else {
        ""
    };
    // Logged-in account (multi-login providers only) so it's always visible
    // WHICH Claude/Codex account the next turn bills against.
    let account = model
        .split('/')
        .next()
        .filter(|provider| crate::auth::is_multi_account_provider(provider))
        .and_then(|provider| crate::auth::active_account_email(config, provider))
        .map(|email| format!("  ·  {email}"))
        .unwrap_or_default();
    // Cost ($) intentionally omitted — the per-model pricing isn't reliable yet.
    // ↑ folds cache-creation tokens into input: on cache-heavy providers the raw
    // input_tokens is a tiny remainder (↑2), which reads as "usage not counted".
    format!(
        " {plan}{project}{branch}  ·  ↑{} ↓{} R{}{context}  ·  {model}{account}",
        fmt_tokens(usage.input + usage.cache_write),
        fmt_tokens(usage.output),
        fmt_tokens(usage.cache_read),
    )
}

/// Centered top-bar title: the session/task name, else the project folder.
fn title_line(store: &SessionStore, config: &AppConfig) -> String {
    let base = store
        .session()
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            config
                .cwd
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    // Show the standing "modes" so you always know what's active — including
    // WHO the agent currently is (adopted persona).
    let mut badges: Vec<String> = Vec::new();
    if let Some(persona) = crate::personas::effective_persona(config) {
        badges.push(
            format!("{} {}", persona.emoji, persona.name)
                .trim()
                .to_string(),
        );
    }
    if crate::commands::plan_mode_active() {
        badges.push("PLAN".to_string());
    }
    if WORKTREE_ORIGIN.lock().map(|g| g.is_some()).unwrap_or(false) {
        badges.push("WORKTREE".to_string());
    }
    if crate::commands::current_goal(config).is_some() {
        badges.push("GOAL".to_string());
    }
    let version = env!("CARGO_PKG_VERSION");
    let platform = crate::update::target_key().unwrap_or(std::env::consts::OS);
    if badges.is_empty() {
        format!("bbarit-oss v{version} ({platform}) · {base}")
    } else {
        format!(
            "bbarit-oss v{version} ({platform}) · {base}   ⟪{}⟫",
            badges.join(" · ")
        )
    }
}

/// The badge for a running mode-turn (harness / multi-job / …), shown in the
/// working bar so you can see which action you started. None = a plain message.
fn turn_mode_label(input: &str) -> Option<&'static str> {
    match input.split_whitespace().next().unwrap_or("") {
        "/harness" | "/build" | "/team" => Some("HARNESS"),
        "/orchestrate" => Some("MULTI-JOB"),
        "/goal" => Some("GOAL"),
        "/loop" => Some("LOOP"),
        "/batch" => Some("BATCH"),
        "/review" => Some("REVIEW"),
        "/bugfix" => Some("BUGFIX"),
        "/improve" | "/autoimprove" | "/upgrade" => Some("IMPROVE"),
        "/plan" => Some("PLAN"),
        _ => None,
    }
}

/// Compact token count: 21340 → "21k", 679 → "679".
fn fmt_tokens(n: usize) -> String {
    if n >= 1000 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

/// Current git branch from `.git/HEAD` (fast, dep-free), if in a repo.
fn git_branch(cwd: &std::path::Path) -> Option<String> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let head = d.join(".git").join("HEAD");
        if let Ok(text) = std::fs::read_to_string(&head) {
            let text = text.trim();
            return Some(
                text.strip_prefix("ref: refs/heads/")
                    .unwrap_or(text)
                    .chars()
                    .take(24)
                    .collect(),
            );
        }
        dir = d.parent();
    }
    None
}

/// Context usage as (percent, window) from the last turn's input tokens vs the
/// model context window, if known.
fn context_usage(store: &SessionStore, registry: &Registry) -> Option<(usize, usize)> {
    let (model_ref, usage) = store.last_token_usage()?;
    let window = registry
        .resolve_reference_with_thinking(model_ref)?
        .model
        .context_window? as usize;
    if window == 0 {
        return None;
    }
    let used = usage.input + usage.cache_read;
    let percent = ((used as f64 / window as f64) * 100.0).round() as usize;
    Some((percent, window))
}

/// The BBARIT AGENT OSS startup splash. Plain 1-cell ASCII (figlet "Standard")
/// so it tiles cleanly in any terminal font — block-drawing glyphs garble in
/// some fonts.
const BBARIT_LOGO: &str = r" ____  ____    _    ____  ___ _____
| __ )| __ )  / \  |  _ \|_ _|_   _|
|  _ \|  _ \ / _ \ | |_) || |  | |
| |_) | |_) / ___ \|  _ < | |  | |
|____/|____/_/   \_\_| \_\___| |_|
    _      ____  _____  _   _  _____     ___   ____   ____
   / \    / ___|| ____|| \ | ||_   _|   / _ \ / ___| / ___|
  / _ \  | |  _ |  _|  |  \| |  | |    | | | |\___ \ \___ \
 / ___ \ | |_| || |___ | |\  |  | |    | |_| | ___) | ___) |
/_/   \_\ \____||_____||_| \_|  |_|     \___/ |____/ |____/";

/// Long tool output is folded to this many lines unless tools are expanded.
const TOOL_FOLD_LINES: usize = 16;

/// `/lens` submitted as a regular agent turn: the agent fetches the diff with
/// its own tools, so the review streams with the normal working UI instead of
/// blocking the UI thread on a one-off completion.
const LENS_TURN_PROMPT: &str = "🔎 lens: review my UNCOMMITTED git changes for quality. Run \
`git diff`, `git diff --staged`, and `git status --porcelain` yourself, then report concrete \
bugs, missing edge cases, security issues, and quality problems introduced by the change — \
each with file:line, ordered by severity, concise. If there are no uncommitted changes, or \
the change looks good, say so briefly.";

/// Neutral frame color for message cards — recedes so the role-colored header
/// and the content stand out. Rgb stays consistent across light/dark themes.
const CARD_FRAME: Color = Color::Rgb(0x6a, 0x6a, 0x6a);
/// Brand green for the assistant's role header.
const AGENT_GREEN: Color = Color::Rgb(0x3f, 0xb9, 0x50);

// ── Design tokens (D1) ──────────────────────────────────────────────────────
// The whole chrome draws from this small curated palette so the screen reads
// as ONE designed surface instead of ad-hoc ANSI colors. Every color is Rgb
// (truecolor) so it renders identically on light and dark terminal themes.
// Escape hatch: to change the tone, tweak the constants below and the whole UI follows.
/// BBARIT brand red — logo, brand chip. Matches the banner art color.
const BRAND_RED: Color = Color::Rgb(0xE0, 0x52, 0x52);
/// Primary interactive accent — input focus, selection, keys, user role.
const ACCENT: Color = Color::Rgb(0x4F, 0xC5, 0xE0);
/// Standing-mode badges (⟪PLAN⟫, queued) — warm, distinct from status colors.
const GOLD: Color = Color::Rgb(0xE3, 0xB4, 0x5F);
/// Explicit fg/bg pair for the selected row in menus — both sides are fixed
/// so selection contrast never depends on the terminal palette.
const SEL_FG: Color = Color::Rgb(0xEA, 0xF7, 0xFB);
const SEL_BG: Color = Color::Rgb(0x14, 0x4A, 0x5E);

/// The FULL transcript as folded lines — used by the copy path, which needs
/// every row to slice a selection out. The render path does NOT use this: it
/// materializes only the visible window (see `render`). Lines come from the
/// same per-entry cache, so indices here match what was drawn.
fn transcript_lines(
    entries: &[Entry],
    partial: Option<&str>,
    width: usize,
    tools_expanded: bool,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut task_no = 0usize;
    for entry in entries {
        // Task (turn) separation: before each user instruction insert a "── task N ──" divider
        // to visually group one instruction with its tools/responses.
        if entry.kind == Kind::User {
            task_no += 1;
            out.push(turn_divider(task_no, width));
        }
        out.extend(entry_lines(entry, width, tools_expanded).iter().cloned());
    }
    if let Some(text) = partial
        && !text.is_empty()
    {
        out.extend(live_partial_lines(text, width, tools_expanded));
    }
    out
}

/// The live streaming entry changes every token — no point caching it. Folded
/// here so its rows count 1:1 against the transcript scroll math.
fn live_partial_lines(text: &str, width: usize, tools_expanded: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    push_entry(&mut lines, Kind::Assistant, text, width, tools_expanded);
    fold_overwide_lines(lines, width)
}

/// A dim "──── Task N ────" divider that separates one user instruction (and
/// the tools/replies it triggered) from the next — the transcript reads as
/// discrete tasks instead of one endless stream.
fn turn_divider(task_no: usize, width: usize) -> Line<'static> {
    let label = format!(" ✦ Task {task_no} ");
    let label_w = UnicodeWidthStr::width(label.as_str());
    let total = width.max(label_w + 4);
    let left = 3usize;
    let right = total.saturating_sub(left + label_w);
    Line::from(vec![
        Span::styled("─".repeat(left), Style::new().fg(CARD_FRAME)),
        Span::styled(label, Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("─".repeat(right), Style::new().fg(CARD_FRAME)),
    ])
}

/// Per-entry render cache. The TUI redraws constantly (streaming ticks at
/// ~12fps); without caching, syntect re-highlights EVERY card on EVERY frame
/// and the UI crawls (observed live: "unusably slow").
///
/// Two maps, both keyed by (entry id, width, tools_expanded) — the id is
/// stable and entries are immutable, so no per-frame hashing of the text:
/// - `lines`: the folded lines, shared via Rc (the render path clones only
///   the rows actually on screen). Bounded: old entries age out by id — never
///   wholesale-cleared, which used to trigger full re-highlight storms once
///   a long session passed the old 512-entry bound.
/// - `counts`: row count per entry, kept for EVERY entry (tiny) so the scroll
///   math never has to materialize off-screen entries at all.
struct EntryRenderCache {
    lines: std::collections::HashMap<(u64, usize, bool), Rc<Vec<Line<'static>>>>,
    counts: std::collections::HashMap<(u64, usize, bool), usize>,
}

thread_local! {
    static ENTRY_RENDER_CACHE: std::cell::RefCell<EntryRenderCache> =
        std::cell::RefCell::new(EntryRenderCache {
            lines: std::collections::HashMap::new(),
            counts: std::collections::HashMap::new(),
        });
}

fn entry_lines(entry: &Entry, width: usize, tools_expanded: bool) -> Rc<Vec<Line<'static>>> {
    let key = (entry.id, width, tools_expanded);
    if let Some(hit) = ENTRY_RENDER_CACHE.with(|cache| cache.borrow().lines.get(&key).cloned()) {
        return hit;
    }
    let mut lines = Vec::new();
    push_entry(&mut lines, entry.kind, &entry.text, width, tools_expanded);
    // Fold once at build time so per-frame code never re-scans every span of
    // every line; folded rows are also what the scroll math counts.
    let lines = Rc::new(fold_overwide_lines(lines, width));
    ENTRY_RENDER_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.counts.insert(key, lines.len());
        if cache.counts.len() > 100_000 {
            cache.counts.clear();
        }
        if cache.lines.len() > 1024 {
            let cutoff = entry.id.saturating_sub(512);
            cache
                .lines
                .retain(|(id, w, _), _| *id >= cutoff && *w == width);
        }
        cache.lines.insert(key, lines.clone());
    });
    lines
}

fn entry_line_count(entry: &Entry, width: usize, tools_expanded: bool) -> usize {
    let key = (entry.id, width, tools_expanded);
    if let Some(count) = ENTRY_RENDER_CACHE.with(|cache| cache.borrow().counts.get(&key).copied()) {
        return count;
    }
    entry_lines(entry, width, tools_expanded).len()
}

fn push_entry(
    out: &mut Vec<Line<'static>>,
    kind: Kind,
    text: &str,
    width: usize,
    tools_expanded: bool,
) {
    // The startup banner has no role prefix: render its art lines in the brand
    // red (BOLD), with a yellow "BETA" badge beside the middle line. Color::Rgb
    // stays vivid on both light and dark themes.
    if kind == Kind::Banner {
        let lines: Vec<&str> = text.split('\n').collect();
        let mid = lines.len() / 2;
        for (i, line) in lines.iter().enumerate() {
            let mut spans = vec![Span::styled(
                line.to_string(),
                Style::new().fg(BRAND_RED).add_modifier(Modifier::BOLD),
            )];
            if i == mid {
                spans.push(Span::styled(
                    "   BETA".to_string(),
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
                ));
            }
            out.push(Line::from(spans));
        }
        return;
    }
    // System notes stay quiet: unframed, dimmed, wrapped — they're status, not
    // conversation, so they shouldn't compete with the message cards.
    if kind == Kind::System {
        for line in text.split('\n') {
            let spans = vec![Span::styled(line.to_string(), Style::new().fg(CARD_FRAME))];
            for row in wrap_spans(&spans, width.max(1)) {
                out.push(Line::from(row));
            }
        }
        out.push(Line::from(String::new()));
        return;
    }

    // Assistant / User / Tool render as a rounded card: role-colored header on
    // the top border, content boxed inside. Tool headers carry the ACTUAL tool
    // name and status color (⚙ bash yellow while running, ✓ green done, ✗ red
    // failed) instead of a generic magenta "Tool".
    // For tool cards, the header also carries a dim one-line "what it did"
    // preview so each step reads at a glance without expanding.
    let mut tool_preview = String::new();
    let (icon, name, color) = match kind {
        Kind::User => ("❯".to_string(), "You".to_string(), ACCENT),
        Kind::Tool => {
            let first = text.lines().next().unwrap_or("");
            let (icon, color) = if first.starts_with('✗') {
                ("✗", Color::Red)
            } else if first.starts_with('✓') {
                ("✓", Color::Green)
            } else {
                ("⚙", Color::Yellow)
            };
            let rest = first.trim_start_matches(['⚙', '✓', '✗']).trim_start();
            let tool_name = rest
                .split_whitespace()
                .next()
                .unwrap_or("tool")
                .trim_end_matches(':');
            // Everything after the tool name = the action (file/command/query).
            let action = rest[tool_name.len().min(rest.len())..].trim();
            tool_preview = action.chars().take(64).collect();
            (icon.to_string(), tool_name.to_string(), color)
        }
        _ => ("●".to_string(), "BBARIT".to_string(), AGENT_GREEN),
    };
    let mut header = vec![Span::styled(
        format!("{icon} {name}"),
        Style::new().fg(color).add_modifier(Modifier::BOLD),
    )];
    if !tool_preview.is_empty() {
        header.push(Span::styled(
            format!("  {tool_preview}"),
            Style::new().fg(CARD_FRAME),
        ));
    }
    let body: Vec<Vec<Span<'static>>> = match kind {
        // Assistant replies are markdown: reuse the markdown styler (headings,
        // code fences, lists, inline code) then card-wrap the resulting spans.
        Kind::Assistant => {
            let mut md = Vec::new();
            append_markdown(&mut md, text);
            md.into_iter().map(|line| line.spans).collect()
        }
        Kind::User => text
            .split('\n')
            .map(|line| vec![Span::styled(line.to_string(), Style::new().fg(ACCENT))])
            .collect(),
        Kind::Tool => {
            let lines: Vec<&str> = text.split('\n').collect();
            // Finished tool cards fold to a short preview to keep the
            // transcript compact (Ctrl+T expands) — but FAILURES stay fully
            // visible: a folded error reads like success at a glance.
            let has_error = lines.iter().any(|line| {
                let t = line.trim_start();
                t.starts_with("✗") || t.starts_with("Tool error:")
            });
            let show = if tools_expanded || has_error {
                lines.len()
            } else {
                lines.len().min(TOOL_FOLD_LINES)
            };
            // Per-card syntax highlighter, keyed off the file the tool touched
            // ("✓ read src/theme.ts …" → TypeScript). Anchor-gutter content
            // lines then render with real syntax colors.
            let file_hint = lines
                .first()
                .and_then(|first| {
                    first
                        .trim_start_matches(['⚙', '✓', '✗'])
                        .split_whitespace()
                        .nth(1)
                })
                .unwrap_or("");
            let mut card_hl = crate::syntax::Highlighter::new(file_hint);
            let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
            let mut index = 0usize;
            while index < show {
                let line = lines[index];
                let lead = line.trim_start();
                // Adjacent `- old` / `+ new` pair (edit previews): word-level
                // inverse — only the changed middle is REVERSED.
                if let Some(old_body) = lead.strip_prefix("- ")
                    && index + 1 < show
                    && let next = lines[index + 1]
                    && let next_lead = next.trim_start()
                    && let Some(new_body) = next_lead.strip_prefix("+ ")
                {
                    let old_indent = &line[..line.len() - lead.len()];
                    let new_indent = &next[..next.len() - next_lead.len()];
                    let (old_spans, new_spans) = crate::syntax::intra_line_diff(old_body, new_body);
                    let mut removed = vec![Span::styled(
                        format!("{old_indent}- "),
                        Style::new().fg(Color::Red),
                    )];
                    removed.extend(old_spans);
                    rows.push(removed);
                    let mut added = vec![Span::styled(
                        format!("{new_indent}+ "),
                        Style::new().fg(Color::Green),
                    )];
                    added.extend(new_spans);
                    rows.push(added);
                    index += 2;
                    continue;
                }
                rows.push(style_tool_line_hl(line, Some(&mut card_hl)));
                index += 1;
            }
            if show < lines.len() {
                rows.push(vec![Span::styled(
                    format!("… {} more lines — Ctrl+T to expand", lines.len() - show),
                    Style::new().fg(CARD_FRAME).add_modifier(Modifier::ITALIC),
                )]);
            }
            rows
        }
        _ => Vec::new(),
    };
    // Error tool cards get a red frame too — the state is visible even when
    // the card is scrolled past quickly (border color encodes status).
    let frame_color = if kind == Kind::Tool && color == Color::Red {
        Color::Red
    } else {
        CARD_FRAME
    };
    push_card(out, header, &body, width, frame_color);
    out.push(Line::from(String::new()));
}

/// Color one tool-transcript line by what it is, so the card reads at a
/// glance instead of as a wall of white. Two-tier de-emphasis: status glyphs
/// carry color (⚙ running yellow, ✓ done green + receding text, ✗ error red),
/// diff lines go green/red, harness hints and the anchor gutter go dim, and
/// plain output sits in muted gray so the assistant's prose stays dominant.
#[cfg(test)]
fn style_tool_line(line: &str) -> Vec<Span<'static>> {
    style_tool_line_hl(line, None)
}

/// Like [`style_tool_line`], with an optional per-card syntax highlighter for
/// anchor-gutter content lines (`12|ab fn main() {` → real code colors).
fn style_tool_line_hl(
    line: &str,
    highlighter: Option<&mut crate::syntax::Highlighter>,
) -> Vec<Span<'static>> {
    let dim = Style::new().fg(CARD_FRAME);
    let muted = Style::new().fg(Color::Gray);
    if let Some(rest) = line.strip_prefix("⚙ ") {
        return vec![
            Span::styled("⚙ ".to_string(), Style::new().fg(Color::Yellow)),
            Span::styled(rest.to_string(), Style::new().fg(Color::Yellow)),
        ];
    }
    if let Some(rest) = line.strip_prefix("✓ ") {
        return vec![
            Span::styled(
                "✓ ".to_string(),
                Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_string(), muted),
        ];
    }
    if let Some(rest) = line.strip_prefix("✗ ") {
        return vec![
            Span::styled(
                "✗ ".to_string(),
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_string(), Style::new().fg(Color::Red)),
        ];
    }
    let trimmed = line.trim_start();
    // Todo checklist rows (indented `✓ done / ▶ doing / ○ pending`): status
    // marker colored, done items recede, the active item pops.
    if !line.starts_with(['✓', '▶', '○'])
        && let Some(marker) = trimmed.chars().next()
        && matches!(marker, '✓' | '▶' | '○')
    {
        let indent = &line[..line.len() - trimmed.len()];
        let rest = trimmed[marker.len_utf8()..].to_string();
        let (marker_style, text_style) = match marker {
            '✓' => (Style::new().fg(Color::Green), dim),
            '▶' => (
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            _ => (muted, muted),
        };
        return vec![
            Span::raw(indent.to_string()),
            Span::styled(marker.to_string(), marker_style),
            Span::styled(rest, text_style),
        ];
    }
    if trimmed.starts_with("Tool error:")
        || trimmed.starts_with("Command exited with code")
        || trimmed.starts_with("Command timed out")
        || trimmed.starts_with("Blocked:")
    {
        return vec![Span::styled(line.to_string(), Style::new().fg(Color::Red))];
    }
    // Diff coloring (edit previews `+ new` / `- old` AND raw git-diff output):
    // file headers dim, hunk headers cyan, additions green, deletions red.
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff --git") {
        return vec![Span::styled(
            line.to_string(),
            dim.add_modifier(Modifier::BOLD),
        )];
    }
    if line.starts_with("@@") {
        return vec![Span::styled(line.to_string(), Style::new().fg(Color::Cyan))];
    }
    if trimmed.starts_with("+ ") || (line.starts_with('+') && !line.starts_with("++")) {
        return vec![Span::styled(
            line.to_string(),
            Style::new().fg(Color::Green),
        )];
    }
    if trimmed.starts_with("- ") || (line.starts_with('-') && !line.starts_with("--")) {
        return vec![Span::styled(line.to_string(), Style::new().fg(Color::Red))];
    }
    // Bulleted rows (wiki lists, summaries): cyan bullet, readable text.
    if let Some(rest) = trimmed.strip_prefix("• ") {
        let indent = &line[..line.len() - trimmed.len()];
        return vec![
            Span::raw(indent.to_string()),
            Span::styled("• ".to_string(), Style::new().fg(Color::Cyan)),
            Span::styled(rest.to_string(), Style::new().fg(Color::Reset)),
        ];
    }
    // Harness annotations ([note: …], [Showing lines …], truncation, ⚠) and
    // ellipsis hints recede to dim italic.
    if trimmed.starts_with('[') || trimmed.starts_with('…') || trimmed.starts_with('⚠') {
        return vec![Span::styled(
            line.to_string(),
            dim.add_modifier(Modifier::ITALIC),
        )];
    }
    // Anchor gutter `1268|fi content` — dim the gutter; the content gets real
    // syntax highlighting when the card knows its file type.
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line.as_bytes()[digits..];
        if rest.len() >= 4
            && rest[0] == b'|'
            && rest[1].is_ascii_lowercase()
            && rest[2].is_ascii_lowercase()
            && rest[3] == b' '
        {
            let mut spans = vec![Span::styled(line[..digits + 4].to_string(), dim)];
            let content = &line[digits + 4..];
            match highlighter {
                Some(hl) if hl.is_active() => spans.extend(hl.line(content)),
                _ => spans.push(Span::styled(
                    content.to_string(),
                    Style::new().fg(Color::Reset),
                )),
            }
            return spans;
        }
    }
    vec![Span::styled(line.to_string(), muted)]
}

/// Draw a rounded card: `╭─ <header> ─…─╮`, each body line boxed as
/// `│ <content> │` (wrapped + right-padded so the border aligns), then
/// `╰──…──╯`. Falls back to plain wrapped lines when the pane is too narrow.
fn push_card(
    out: &mut Vec<Line<'static>>,
    header: Vec<Span<'static>>,
    body: &[Vec<Span<'static>>],
    width: usize,
    frame_color: Color,
) {
    let width = width.max(1);
    let frame = Style::new().fg(frame_color);
    if width < 8 {
        out.push(Line::from(header));
        for line in body {
            for row in wrap_spans(line, width) {
                out.push(Line::from(row));
            }
        }
        return;
    }
    let inner = width - 4; // "│ " + content + " │"
    // Top border sized to exactly `width`: "╭─ " (3) + header + " " + fill + "╮".
    let header_w: usize = header
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    let fill = width.saturating_sub(3 + header_w + 1 + 1);
    let mut top = vec![Span::styled("╭─ ".to_string(), frame)];
    top.extend(header);
    top.push(Span::styled(format!(" {}╮", "─".repeat(fill)), frame));
    out.push(Line::from(top));
    if body.is_empty() {
        out.push(card_body_row(Vec::new(), inner, frame));
    }
    for line in body {
        for row in wrap_spans(line, inner) {
            out.push(card_body_row(row, inner, frame));
        }
    }
    out.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(width - 2)),
        frame,
    )));
}

/// Wrap one boxed content row: right-pad to `inner` display columns, then flank
/// with the vertical frame so the right border lines up.
fn card_body_row(mut row: Vec<Span<'static>>, inner: usize, frame: Style) -> Line<'static> {
    let used: usize = row
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if used < inner {
        row.push(Span::raw(" ".repeat(inner - used)));
    }
    let mut spans = vec![Span::styled("│ ".to_string(), frame)];
    spans.extend(row);
    spans.push(Span::styled(" │".to_string(), frame));
    Line::from(spans)
}

/// Split any line wider than `width` display columns into multiple lines so the
/// transcript's scroll math (1 line = 1 row) holds even for lines that weren't
/// pre-wrapped at layout time (markdown text, system notes, CJK-heavy output).
fn fold_overwide_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let w: usize = line
            .spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        if w <= width {
            out.push(line);
        } else {
            let style = line.style;
            for row in wrap_spans(&line.spans, width) {
                let mut folded = Line::from(row);
                folded.style = style;
                out.push(folded);
            }
        }
    }
    out
}

/// Break styled spans into visual rows no wider than `width` display columns,
/// splitting at char boundaries so CJK / wide glyphs are measured correctly and
/// per-char style is preserved. Always yields at least one (possibly empty) row.
fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut row_w = 0usize;
    let mut buf = String::new();
    let mut buf_style = Style::default();
    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if row_w + cw > width && (row_w > 0 || !buf.is_empty()) {
                if !buf.is_empty() {
                    row.push(Span::styled(std::mem::take(&mut buf), buf_style));
                }
                rows.push(std::mem::take(&mut row));
                row_w = 0;
            }
            if !buf.is_empty() && buf_style != style {
                row.push(Span::styled(std::mem::take(&mut buf), buf_style));
            }
            buf_style = style;
            buf.push(ch);
            row_w += cw;
        }
    }
    if !buf.is_empty() {
        row.push(Span::styled(buf, buf_style));
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

/// Render markdown `text` as styled ratatui lines (one source line → one line,
/// so transcript scroll math stays correct).
fn append_markdown(out: &mut Vec<Line<'static>>, text: &str) {
    // A live syntect highlighter for the current code fence: created from the
    // fence language (```rust), stateful across the block so multi-line
    // strings/comments color correctly. Unknown language → light fallback.
    let mut fence: Option<crate::syntax::Highlighter> = None;
    let mut code_line = 0usize;
    for raw in text.split('\n') {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            if fence.is_none() {
                let lang = trimmed.trim_start_matches('`').trim();
                fence = Some(crate::syntax::Highlighter::new(lang));
                code_line = 0;
            } else {
                fence = None;
            }
            // Fence markers recede — the code inside is the content.
            out.push(Line::from(Span::styled(
                raw.to_string(),
                Style::new().fg(CARD_FRAME),
            )));
            continue;
        }
        if let Some(highlighter) = fence.as_mut() {
            // Numbered code lines: dim gutter + real syntax highlighting.
            code_line += 1;
            let mut spans = vec![Span::styled(
                format!("{code_line:>4} "),
                Style::new().fg(CARD_FRAME),
            )];
            if highlighter.is_active() {
                spans.extend(highlighter.line(raw));
            } else {
                spans.extend(highlight_code_line(raw));
            }
            out.push(Line::from(spans));
            continue;
        }
        // Tool activity lines: ✓ green (success), ✗ red (error), ⚙ gray (running).
        if let Some(marker) = trimmed.chars().next()
            && matches!(marker, '✓' | '✗' | '⚙')
        {
            let color = match marker {
                '✓' => Color::Green,
                '✗' => Color::Red,
                _ => Color::Reset,
            };
            out.push(Line::from(Span::styled(
                raw.to_string(),
                Style::new().fg(color),
            )));
            continue;
        }
        // Headings share the accent (hierarchy comes from weight/underline,
        // not a rainbow): # accent+underline, ## accent, ### default bold.
        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::new().add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::new()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
            continue;
        }
        // Blockquote.
        if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push(Line::from(Span::styled(
                format!("┃ {rest}"),
                Style::new().fg(Color::Green).add_modifier(Modifier::ITALIC),
            )));
            continue;
        }
        // Horizontal rule.
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push(Line::from(Span::styled(
                "─".repeat(40),
                Style::new().fg(CARD_FRAME),
            )));
            continue;
        }
        // Bullet (cyan) or numbered (cyan number) list item.
        let (bullet, content) = if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            (Some("  • ".to_string()), rest)
        } else if let Some((number, rest)) = split_numbered(trimmed) {
            (Some(format!("  {number}. ")), rest)
        } else {
            (None, raw)
        };
        let mut spans = Vec::new();
        if let Some(bullet) = bullet {
            spans.push(Span::styled(bullet, Style::new().fg(ACCENT)));
        }
        spans.extend(inline_spans(content));
        out.push(Line::from(spans));
    }
}

/// Common keywords across languages, highlighted in code blocks.
const CODE_KEYWORDS: &[&str] = &[
    "def",
    "class",
    "function",
    "fn",
    "func",
    "return",
    "if",
    "else",
    "elif",
    "for",
    "while",
    "loop",
    "break",
    "continue",
    "import",
    "from",
    "export",
    "const",
    "let",
    "var",
    "public",
    "private",
    "protected",
    "static",
    "void",
    "true",
    "false",
    "null",
    "nil",
    "none",
    "None",
    "True",
    "False",
    "self",
    "this",
    "super",
    "new",
    "async",
    "await",
    "try",
    "catch",
    "except",
    "finally",
    "with",
    "as",
    "in",
    "is",
    "and",
    "or",
    "not",
    "struct",
    "enum",
    "impl",
    "trait",
    "pub",
    "use",
    "mod",
    "match",
    "case",
    "switch",
    "lambda",
    "yield",
    "raise",
    "throw",
    "type",
    "interface",
    "extends",
    "implements",
    "package",
    "namespace",
    "module",
];

/// Lightweight, language-agnostic syntax highlight for one code line: strings,
/// comments, numbers and common keywords get distinct (non-yellow) colors so
/// source is readable. Not a real parser — good enough to tell tokens apart.
fn highlight_code_line(line: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, spans: &mut Vec<Span<'static>>| {
        if !buf.is_empty() {
            spans.push(Span::styled(
                std::mem::take(buf),
                Style::new().fg(Color::Reset),
            ));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Line comments: // # -- (rest of line).
        let two: String = chars[i..].iter().take(2).collect();
        if two == "//" || two == "--" || c == '#' {
            flush(&mut buf, &mut spans);
            let rest: String = chars[i..].iter().collect();
            spans.push(Span::styled(
                rest,
                Style::new().fg(Color::Blue).add_modifier(Modifier::ITALIC),
            ));
            break;
        }
        // Strings: " ' ` (to matching close on the same line).
        if c == '"' || c == '\'' || c == '`' {
            flush(&mut buf, &mut spans);
            let quote = c;
            let mut s = String::from(c);
            i += 1;
            while i < chars.len() {
                s.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    s.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push(Span::styled(s, Style::new().fg(Color::Green)));
            continue;
        }
        // Numbers.
        if c.is_ascii_digit()
            && buf
                .chars()
                .last()
                .map(|p| !p.is_alphanumeric() && p != '_')
                .unwrap_or(true)
        {
            flush(&mut buf, &mut spans);
            let mut n = String::new();
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                n.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(n, Style::new().fg(Color::Magenta)));
            continue;
        }
        // Identifiers / keywords.
        if c.is_alphabetic() || c == '_' {
            let mut word = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                word.push(chars[i]);
                i += 1;
            }
            if CODE_KEYWORDS.contains(&word.as_str()) {
                flush(&mut buf, &mut spans);
                spans.push(Span::styled(
                    word,
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
            } else {
                buf.push_str(&word);
            }
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut spans);
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

/// Parse inline markdown (`**bold**` and `` `code` ``) into styled spans.
fn inline_spans(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let flush = |buffer: &mut String, spans: &mut Vec<Span<'static>>| {
        if !buffer.is_empty() {
            spans.push(Span::raw(std::mem::take(buffer)));
        }
    };
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`'
            && let Some(end) = chars[index + 1..].iter().position(|&c| c == '`')
        {
            let end = index + 1 + end;
            flush(&mut buffer, &mut spans);
            let code: String = chars[index + 1..end].iter().collect();
            spans.push(Span::styled(code, Style::new().fg(Color::Cyan)));
            index = end + 1;
            continue;
        }
        if chars[index] == '*'
            && index + 1 < chars.len()
            && chars[index + 1] == '*'
            && let Some(end) = find_double_star(&chars, index + 2)
        {
            flush(&mut buffer, &mut spans);
            let bold: String = chars[index + 2..end].iter().collect();
            spans.push(Span::styled(
                bold,
                // Default fg + BOLD so **bold** stays visible on light themes too
                // (hardcoded White was invisible on a light terminal background).
                Style::new().fg(Color::Reset).add_modifier(Modifier::BOLD),
            ));
            index = end + 2;
            continue;
        }
        buffer.push(chars[index]);
        index += 1;
    }
    flush(&mut buffer, &mut spans);
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

/// Split a numbered-list line ("3. text") into ("3", "text").
fn split_numbered(line: &str) -> Option<(String, &str)> {
    let dot = line.find('.')?;
    let number = &line[..dot];
    if number.is_empty() || number.len() > 3 || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let rest = line[dot + 1..].strip_prefix(' ')?;
    Some((number.to_string(), rest))
}

fn find_double_star(chars: &[char], from: usize) -> Option<usize> {
    let mut index = from;
    while index + 1 < chars.len() {
        if chars[index] == '*' && chars[index + 1] == '*' {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    #[test]
    fn format_ago_scales_units() {
        assert_eq!(format_ago(0), "just now");
        assert_eq!(format_ago(59), "just now");
        assert_eq!(format_ago(60), "1m ago");
        assert_eq!(format_ago(3599), "59m ago");
        assert_eq!(format_ago(3600), "1h ago");
        assert_eq!(format_ago(86_399), "23h ago");
        assert_eq!(format_ago(86_400), "1d ago");
        assert_eq!(format_ago(30 * 86_400), "30d ago");
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn feed_all(reasm: &mut PasteReassembly, events: Vec<Event>) -> Vec<Event> {
        let mut out = Vec::new();
        for event in events {
            out.extend(reasm.feed(event));
        }
        out.extend(reasm.flush());
        out
    }

    fn marker(chars: [char; 5]) -> Vec<Event> {
        let mut out = vec![key(KeyCode::Esc)];
        out.extend(chars.map(|c| key(KeyCode::Char(c))));
        out
    }

    #[test]
    fn worktree_dir_name_sanitizes_and_rejects_traversal() {
        assert_eq!(
            worktree_dir_name("feature/x y"),
            Some("feature-x-y".to_string())
        );
        assert_eq!(worktree_dir_name("fix_1.2"), Some("fix_1.2".to_string()));
        // "." and ".." would resolve to `.bbarit/worktrees` itself or above it.
        assert_eq!(worktree_dir_name("."), None);
        assert_eq!(worktree_dir_name(".."), None);
        assert_eq!(worktree_dir_name(""), None);
    }

    #[test]
    fn expand_home_resolves_tilde_prefix() {
        if let Some(home) = dirs_next::home_dir() {
            assert_eq!(expand_home("~"), home);
            assert_eq!(expand_home("~/projects/foo"), home.join("projects/foo"));
        }
        assert_eq!(expand_home("plain/path"), PathBuf::from("plain/path"));
        // Only a leading `~` component expands — `~user` stays literal.
        assert_eq!(expand_home("~user/foo"), PathBuf::from("~user/foo"));
    }

    #[test]
    fn bracketed_paste_reassembles_multiline_block() {
        // Windows ConPTY delivers ESC[200~Hi\rBye ESC[201~ as single keys;
        // the whole block must come back as ONE paste, not line-by-line Enters.
        let mut events = marker(PASTE_OPEN);
        events.extend("Hi".chars().map(|c| key(KeyCode::Char(c))));
        events.push(key(KeyCode::Enter));
        events.extend("Bye".chars().map(|c| key(KeyCode::Char(c))));
        events.extend(marker(PASTE_CLOSE));
        let out = feed_all(&mut PasteReassembly::default(), events);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(matches!(&out[0], Event::Paste(text) if text == "Hi\nBye"));
    }

    #[test]
    fn login_ack_accepts_enter_esc_and_ctrl_c() {
        assert!(is_login_ack_key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(is_login_ack_key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(is_login_ack_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        // Typing text must not dismiss the prompt; only Ctrl+C 'c' counts.
        assert!(!is_login_ack_key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!is_login_ack_key(KeyCode::Char('c'), KeyModifiers::NONE));
    }

    #[test]
    fn lone_esc_stays_a_real_esc_press() {
        let out = feed_all(&mut PasteReassembly::default(), vec![key(KeyCode::Esc)]);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Event::Key(k) if k.code == KeyCode::Esc));
    }

    #[test]
    fn non_marker_after_esc_replays_literally() {
        let out = feed_all(
            &mut PasteReassembly::default(),
            vec![
                key(KeyCode::Esc),
                key(KeyCode::Char('[')),
                key(KeyCode::Char('A')),
            ],
        );
        let codes: Vec<KeyCode> = out
            .iter()
            .filter_map(|e| match e {
                Event::Key(k) => Some(k.code),
                _ => None,
            })
            .collect();
        assert_eq!(
            codes,
            vec![KeyCode::Esc, KeyCode::Char('['), KeyCode::Char('A')]
        );
    }

    #[test]
    fn unterminated_paste_flushes_collected_text() {
        let mut events = marker(PASTE_OPEN);
        events.extend("tail".chars().map(|c| key(KeyCode::Char(c))));
        let out = feed_all(&mut PasteReassembly::default(), events);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Event::Paste(text) if text == "tail"));
    }

    #[test]
    fn half_matched_close_marker_is_kept_as_content() {
        // ESC[20X inside the body is literal content, not the close marker.
        let mut events = marker(PASTE_OPEN);
        events.push(key(KeyCode::Esc));
        events.extend("[20".chars().map(|c| key(KeyCode::Char(c))));
        events.push(key(KeyCode::Char('X')));
        events.extend(marker(PASTE_CLOSE));
        let out = feed_all(&mut PasteReassembly::default(), events);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            matches!(&out[0], Event::Paste(text) if text == "\x1b[20X"),
            "{out:?}"
        );
    }

    fn scroll_test_app() -> App {
        App {
            input: String::new(),
            scroll: 0,
            follow: true,
            last_max_scroll: 0,
            status: String::new(),
            entries: Vec::new(),
            selector: None,
            prompt: None,
            queued: Vec::new(),
            tools_expanded: false,
            working: false,
            working_text: String::new(),
            title: String::new(),
            tabs: Vec::new(),
            active_session: String::new(),
            exit: false,
            history: Vec::new(),
            history_pos: None,
            mouse_sel: None,
            mouse_sel_dragged: false,
            last_transcript_area: Rect::new(0, 0, 0, 0),
            last_scroll: 0,
            ctrl_c_at: None,
        }
    }

    #[test]
    fn history_prev_next_navigates_and_returns_to_fresh_line() {
        let mut app = scroll_test_app();
        app.history = vec!["첫 명령".to_string(), "둘째 명령".to_string()];
        app.history_prev();
        assert_eq!(app.input, "둘째 명령");
        app.history_prev();
        assert_eq!(app.input, "첫 명령");
        app.history_prev(); // past the front, stay put
        assert_eq!(app.input, "첫 명령");
        app.history_next();
        assert_eq!(app.input, "둘째 명령");
        app.history_next(); // past the end, a blank new line
        assert_eq!(app.input, "");
        assert_eq!(app.history_pos, None);
    }

    #[test]
    fn record_history_skips_empty_and_consecutive_dupes() {
        let dir = std::env::temp_dir().join(format!("bbarit-hist-rec-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let config = crate::config::AppConfig::for_test(dir.clone());
        let mut app = scroll_test_app();
        app.record_history(&config, "");
        app.record_history(&config, "명령 A");
        app.record_history(&config, "명령 A");
        app.record_history(&config, "명령 B");
        assert_eq!(
            app.history,
            vec!["명령 A".to_string(), "명령 B".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn input_history_roundtrip_persists_multiline_entries() {
        let dir = std::env::temp_dir().join(format!("bbarit-hist-rt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let config = crate::config::AppConfig::for_test(dir.clone());
        append_input_history(&config, "한 줄 명령");
        append_input_history(&config, "여러 줄\n입력도\n보존");
        assert_eq!(
            load_input_history(&config),
            vec![
                "한 줄 명령".to_string(),
                "여러 줄\n입력도\n보존".to_string()
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_smoke_draws_full_chrome() {
        // One full frame through the real render() with every chrome element
        // active (brand chip, badges, divider, cards, working bar, queued,
        // placeholder input, segmented footer) — pins the redesign together
        // and doubles as a visual dump: run with -- --nocapture to eyeball it.
        use ratatui::backend::TestBackend;
        let mut app = scroll_test_app();
        app.title = "sample-project   ⟪PLAN · GOAL⟫".to_string();
        app.status = " sample (main)  ·  ↑12k ↓3k R8k · 42%/200k  ·  claude-fable".to_string();
        app.entries
            .push(Entry::new(Kind::User, "빠릿 디자인 확인".to_string()));
        app.entries.push(Entry::new(
            Kind::Tool,
            "✓ read src/tui.rs — 2 lines\n12|ab fn main() {".to_string(),
        ));
        app.entries.push(Entry::new(
            Kind::Assistant,
            "# 결과\n- 항목 **강조**".to_string(),
        ));
        app.working = true;
        app.working_text = " ⠋ working.  ▰▱▱▱▱  3s   (Esc to cancel)".to_string();
        app.queued.push("다음 지시".to_string());
        let backend = TestBackend::new(100, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &mut app, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        println!("{text}");
        // Wide (CJK) glyphs occupy two buffer cells and the dump pads the
        // second with a blank, so Korean needles match space-insensitively.
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(text.contains("BBARIT"), "brand chip missing");
        assert!(compact.contains("⟪PLAN⟫"), "badge chip missing");
        assert!(compact.contains("✦Task1"), "turn divider missing");
        assert!(text.contains("❯ You"), "user card header missing");
        assert!(compact.contains("working"), "working bar missing");
        assert!(compact.contains("1queued"), "queued strip missing");
        assert!(compact.contains("Askanything"), "placeholder missing");
        assert!(text.contains("claude-fable"), "footer model missing");
    }

    #[test]
    fn follow_shows_bottom_of_tall_korean_response() {
        // Regression (2026-07-05 screenshot): the bottom of the card was cut off on a long reply,
        // with follow=true the last content line and the card's bottom
        // border must be on screen. If full-width (CJK) width calc disagrees with the row count,
        // max_scroll comes up short and the bottom becomes unreachable.
        use ratatui::backend::TestBackend;
        let mut app = scroll_test_app();
        app.entries
            .push(Entry::new(Kind::User, "모하냐구".to_string()));
        let mut body = String::from(
            "지금은 빠릿터미널 프로젝트 컨텍스트에서 대기 중이야.\n방금 자동으로 SEO 알림 스킬 관련 코드가 붙었는데, 네가 원한 작업이 그건지 아직 불명확해.\n\n뭐 할까? 예를 들면:\n",
        );
        for i in 0..12 {
            body.push_str(&format!(
                "- 항목 {i}: 빠릿터미널 버그 수정 그리고 충분히 길어서 접힐 수도 있는 한국어 설명 텍스트\n"
            ));
        }
        body.push_str("마지막줄ENDMARK");
        app.entries.push(Entry::new(Kind::Assistant, body));
        app.follow = true;
        let backend = TestBackend::new(100, 24); // short window → scrolling required
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &mut app, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        println!("{text}");
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.contains("마지막줄ENDMARK"),
            "follow=true인데 응답의 마지막 줄이 화면에 없다 — 하단 잘림 회귀"
        );
        assert!(
            compact.contains("╰"),
            "카드 하단 테두리가 화면에 없다 — 하단 잘림 회귀"
        );
    }

    #[test]
    fn follow_bottom_aligns_short_transcript_above_input() {
        use ratatui::backend::TestBackend;
        let mut app = scroll_test_app();
        app.entries
            .push(Entry::new(Kind::Assistant, "BOTTOM_ALIGN_MARK".to_string()));
        app.follow = true;
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &mut app, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut marker_row = None;
        let mut input_row = None;
        for y in 0..buffer.area.height {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            if row.contains("BOTTOM_ALIGN_MARK") {
                marker_row = Some(y);
            }
            if row.contains("Ask anything") {
                input_row = Some(y);
            }
        }
        let marker_row = marker_row.expect("marker should render");
        let input_row = input_row.expect("input placeholder should render");
        assert!(
            marker_row >= input_row.saturating_sub(5) && marker_row < input_row,
            "short transcript should sit just above input, marker row {marker_row}, input row {input_row}"
        );
    }

    #[test]
    fn transcript_row_mapping_respects_rect_and_scroll() {
        let mut app = scroll_test_app();
        app.last_transcript_area = Rect::new(0, 2, 80, 20); // transcript starts at row 2
        app.last_scroll = 30;
        // Top-left visible cell = absolute row 30; third visible row = 32.
        assert_eq!(app.transcript_row_at(0, 2), Some(30));
        assert_eq!(app.transcript_row_at(10, 4), Some(32));
        // Outside the rect (above / below / right of it) maps to nothing.
        assert_eq!(app.transcript_row_at(0, 1), None);
        assert_eq!(app.transcript_row_at(0, 22), None);
        assert_eq!(app.transcript_row_at(80, 5), None);
        // Drag clamps to the visible range instead of bailing.
        assert_eq!(app.transcript_row_clamped(0), 30);
        assert_eq!(app.transcript_row_clamped(200), 30 + 19);
    }

    #[test]
    fn transcript_selection_text_extracts_and_clamps_rows() {
        let lines: Vec<Line<'static>> = vec![
            Line::from("first  "),
            Line::from("second"),
            Line::from("third"),
        ];
        assert_eq!(transcript_selection_text(&lines, 0, 1), "first\nsecond");
        // Reversed/overflowing indices clamp to the last row.
        assert_eq!(transcript_selection_text(&lines, 2, 99), "third");
        assert_eq!(transcript_selection_text(&[], 0, 5), "");
    }

    #[test]
    fn scroll_up_from_follow_anchors_at_bottom() {
        let mut app = scroll_test_app();
        app.last_max_scroll = 100; // rendered frame said bottom = 100
        app.scroll_up_by(10);
        // One page up from the bottom — NOT a jump to the top (old bug).
        assert!(!app.follow);
        assert_eq!(app.scroll, 90);
        // Top saturates at 0.
        app.scroll_up_by(200);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn scroll_down_reenables_follow_at_bottom() {
        let mut app = scroll_test_app();
        app.last_max_scroll = 20;
        app.scroll_up_by(15); // leave follow at 5
        app.scroll_down_by(10);
        assert_eq!(app.scroll, 15);
        assert!(!app.follow);
        app.scroll_down_by(10); // past the bottom → clamp + follow again
        assert_eq!(app.scroll, 20);
        assert!(app.follow);
        // Scrolling down while already following is a no-op.
        app.scroll_down_by(10);
        assert!(app.follow);
        assert_eq!(app.scroll, 20);
    }

    #[test]
    fn url_brand_strips_scheme_www_and_port() {
        assert_eq!(url_brand("https://www.github.com/a/b?x=1"), "github.com");
        assert_eq!(url_brand("http://localhost:11434/v1"), "localhost");
        assert_eq!(
            url_brand("https://platform.openai.com/docs"),
            "platform.openai.com"
        );
    }

    #[test]
    fn links_grouped_by_brand_with_headers() {
        let entries = vec![
            Entry::new(
                Kind::Assistant,
                "see https://github.com/x and https://github.com/y.".into(),
            ),
            Entry::new(Kind::User, "also https://openai.com/z)".into()),
        ];
        let selector = links_selector(&entries);
        let links: Vec<_> = selector
            .items
            .iter()
            .filter(|(_, command)| command.starts_with("http"))
            .collect();
        let headers: Vec<_> = selector
            .items
            .iter()
            .filter(|(_, command)| command.is_empty())
            .collect();
        assert_eq!(links.len(), 3, "three unique urls");
        assert_eq!(headers.len(), 2, "two brand groups");
        assert!(
            selector.items.iter().any(|(_, command)| command == "@menu"),
            "links selector should provide a path back to the main menu"
        );
        // Trailing punctuation is trimmed from the captured URL.
        assert!(links.iter().any(|(_, c)| c == "https://openai.com/z"));
        // Cursor starts on a selectable row, not a header.
        assert!(!selector.items[selector.cursor].1.is_empty());
        // Headers use friendly brand names.
        assert!(headers.iter().any(|(label, _)| label.contains("GitHub")));
        assert!(headers.iter().any(|(label, _)| label.contains("OpenAI")));
    }

    #[test]
    fn selectors_have_back_or_close_action() {
        let config = AppConfig::for_test(std::env::temp_dir().join("bbarit-selector-back"));
        let registry = Registry::load(&config).unwrap();
        let selectors = vec![
            links_selector(&[]),
            folder_selector(&config),
            provider_selector(&registry, &config),
            model_selector(&registry, &config, None, ""),
            harness_model_selector(&registry, &config, None, false),
            roles_selector(&config),
            thinking_selector(crate::providers::ThinkingLevel::Medium),
            session_selector(&config),
            command_menu(),
            login_selector(&registry),
        ];
        for selector in selectors {
            assert!(
                selector
                    .items
                    .iter()
                    .any(|(_, command)| matches!(command.as_str(), "@menu" | "@close")),
                "{} should have Back to menu or Close menu",
                selector.title
            );
        }
    }

    #[test]
    fn login_selector_offers_kimi_code_api_key_login() {
        let config = AppConfig::for_test(std::env::temp_dir().join("bbarit-login-selector"));
        let registry = Registry::load(&config).unwrap();
        let selector = login_selector(&registry);
        // Providers outside the curated list must still get an API-key entry.
        assert!(
            selector
                .items
                .iter()
                .any(|(_, command)| command.starts_with("/login ")
                    && !command.contains("kimi")
                    && command.ends_with(' ')),
            "login selector should list catalog providers beyond the curated set"
        );
        assert!(
            selector
                .items
                .iter()
                .any(|(_, command)| command == "/login kimi-coding "),
            "login selector should offer Kimi Code"
        );
        assert!(
            selector
                .items
                .iter()
                .any(|(_, command)| command == "/login moonshotai "),
            "login selector should offer Moonshot AI"
        );
        assert_eq!(pretty_provider("kimi-coding"), "Kimi Code");
        assert_eq!(pretty_provider("moonshotai"), "Moonshot AI");
        // The API-key prompt masks the pasted key and shows the friendly name.
        let prompt = Prompt::for_command("/login kimi-coding ");
        assert!(prompt.masked);
        assert!(prompt.title.starts_with("Kimi Code"));
    }

    #[test]
    fn key_only_providers_skip_the_picker_for_a_direct_key_prompt() {
        // OAuth-capable providers keep the picker (browser vs. key choice).
        assert!(provider_supports_browser_login("anthropic"));
        assert!(provider_supports_browser_login("openai-codex"));
        assert!(provider_supports_browser_login("github-copilot"));
        // Key-only providers jump straight to the masked key prompt.
        assert!(!provider_supports_browser_login("kimi-coding"));
        assert!(!provider_supports_browser_login("openrouter"));
        assert!(!provider_supports_browser_login("moonshotai"));
        let prompt = Prompt::for_command("/login kimi-coding ");
        assert!(prompt.masked, "key prompt must be masked");
    }

    #[test]
    fn login_selector_offers_api_key_entries_for_oauth_providers() {
        let config = AppConfig::for_test(std::env::temp_dir().join("bbarit-login-oauth-key"));
        let registry = Registry::load(&config).unwrap();
        let selector = login_selector(&registry);
        // The trailing space opens the masked input overlay instead of
        // launching the browser flow.
        assert!(
            selector
                .items
                .iter()
                .any(|(label, command)| label.contains("Anthropic (API key)")
                    && command == "/login anthropic api-key "),
            "Anthropic should offer an explicit API-key entry"
        );
        assert!(
            selector
                .items
                .iter()
                .any(|(_, command)| command == "/login github-copilot api-key "),
            "GitHub Copilot should offer an explicit API-key entry"
        );
        // A raw key can't drive the Codex OAuth backend, so the OpenAI entry
        // routes to the plain "openai" provider.
        assert!(
            selector
                .items
                .iter()
                .any(|(label, command)| label.contains("Codex API key")
                    && command == "/login openai "),
            "OpenAI API-key entry should target the openai provider"
        );
        // Browser-login entries stay (no trailing space → OAuth flow).
        for command in [
            "/login anthropic",
            "/login openai-codex",
            "/login github-copilot",
        ] {
            assert!(
                selector.items.iter().any(|(_, c)| c == command),
                "browser login entry {command} must survive"
            );
        }
    }

    #[test]
    fn login_api_key_prompt_masks_and_strips_kind_token() {
        let prompt = Prompt::for_command("/login anthropic api-key ");
        assert!(prompt.masked);
        assert_eq!(prompt.title, "Anthropic · paste API key");
        let prompt = Prompt::for_command("/login github-copilot api-key ");
        assert!(prompt.masked);
        assert_eq!(prompt.title, "GitHub Copilot · paste API key");
    }

    #[test]
    fn session_switch_commands_detected_with_and_without_args() {
        for line in [
            "/new",
            "/clone",
            "/clone abc",
            "/fork abc123",
            "/resume abc123",
            "/import ~/session.json",
        ] {
            assert!(is_session_switch_command(line), "{line}");
        }
        for line in ["/resumex", "/sessions", "/model", "resume", "hello /resume"] {
            assert!(!is_session_switch_command(line), "{line}");
        }
    }

    #[test]
    fn tui_only_commands_get_interactive_note_in_line_repl() {
        for line in [
            "/clear",
            "/cls",
            "/menu",
            "/links",
            "/personas",
            "/setup",
            "/folder",
            "/codebase",
        ] {
            assert!(is_interactive_only_command(line), "{line}");
        }
        // Commands the non-TUI layer handles must still reach handle_input.
        for line in [
            "/help",
            "/persona list",
            "/settings",
            "/resume abc",
            "hello",
        ] {
            assert!(!is_interactive_only_command(line), "{line}");
        }
    }

    #[test]
    fn markdown_renders_code_heading_bullet_and_inline() {
        let mut out = Vec::new();
        append_markdown(
            &mut out,
            "# Title\n- item with `code` and **bold**\n```py\nprint(1)\n```",
        );
        // One line per source line (5), keeping scroll math stable.
        assert_eq!(out.len(), 5);
        // Inline parsing splits text/code/bold into multiple spans.
        let bullet_spans = out[1].spans.len();
        assert!(bullet_spans >= 3, "bullet line should have multiple spans");
    }

    #[test]
    fn inline_spans_handle_bold_and_code() {
        let spans = inline_spans("a `b` c **d** e");
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "a b c d e");
        assert!(spans.len() >= 4);
    }

    fn line_width(line: &Line) -> usize {
        line.spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum()
    }

    #[test]
    fn tool_lines_are_status_colored_and_gutter_dimmed() {
        // ✓ line: green glyph, receding text.
        let done = style_tool_line("✓ bash cargo test — 3 lines");
        assert_eq!(done[0].content.as_ref(), "✓ ");
        assert_eq!(done[0].style.fg, Some(Color::Green));
        // ✗ line: red.
        let failed = style_tool_line("✗ bash: Tool error: boom");
        assert_eq!(failed[0].style.fg, Some(Color::Red));
        // Anchor gutter `12|ab content`: gutter dimmed, content plain.
        let anchored = style_tool_line("12|ab fn main() {");
        assert_eq!(anchored[0].content.as_ref(), "12|ab ");
        assert_eq!(anchored[0].style.fg, Some(CARD_FRAME));
        assert_eq!(anchored[1].content.as_ref(), "fn main() {");
        // Diff previews color by sign.
        assert_eq!(
            style_tool_line("  + new_line();")[0].style.fg,
            Some(Color::Green)
        );
        assert_eq!(
            style_tool_line("  - old_line();")[0].style.fg,
            Some(Color::Red)
        );
        // Plain output recedes to muted gray.
        assert_eq!(
            style_tool_line("compiling foo v0.1.0")[0].style.fg,
            Some(Color::Gray)
        );
    }

    #[test]
    fn failed_tool_cards_do_not_fold() {
        // A folded error reads like success — errors must stay fully visible.
        let mut folded = Vec::new();
        let many_ok = (1..=TOOL_FOLD_LINES + 4)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        push_entry(&mut folded, Kind::Tool, &many_ok, 60, false);
        let folded_text: String = folded
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(folded_text.contains("more lines"), "success folds");

        let mut unfolded = Vec::new();
        let with_error = format!("✗ bash: Tool error: boom\n{many_ok}");
        push_entry(&mut unfolded, Kind::Tool, &with_error, 60, false);
        let unfolded_text: String = unfolded
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(!unfolded_text.contains("more lines"), "errors stay open");
        assert!(unfolded_text.contains(&format!("line {}", TOOL_FOLD_LINES + 4)));
    }

    #[test]
    fn card_lines_fill_exact_width_so_borders_align() {
        // Every rendered card line (top border, boxed body — including wrapped
        // and empty lines — and bottom border) must be exactly the pane width,
        // otherwise the right `│`/`╮`/`╯` edge is ragged.
        let width = 40;
        let header = vec![Span::styled("● 빠릿".to_string(), Style::default())];
        let body = vec![
            vec![Span::raw("짧은 한글 줄".to_string())],
            vec![Span::raw("x".repeat(90))], // forces multiple wraps
            vec![Span::raw(String::new())],  // blank interior line
        ];
        let mut out = Vec::new();
        push_card(&mut out, header, &body, width, CARD_FRAME);
        for line in &out {
            assert_eq!(
                line_width(line),
                width,
                "card line not full width: {line:?}"
            );
        }
        assert!(out.first().unwrap().spans[0].content.starts_with('╭'));
        assert!(out.last().unwrap().spans[0].content.starts_with('╰'));
    }

    #[test]
    fn wrap_spans_splits_wide_glyphs_and_keeps_content() {
        // 5 CJK chars = 10 display cols; at width 4 (2 CJK/row) that's 3 rows,
        // and no character is dropped or duplicated.
        let spans = vec![Span::raw("가나다라마".to_string())];
        let rows = wrap_spans(&spans, 4);
        assert_eq!(rows.len(), 3);
        for row in &rows {
            let w: usize = row
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(w <= 4, "row exceeds width: {w}");
        }
        let joined: String = rows
            .iter()
            .flat_map(|r| r.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(joined, "가나다라마");
    }

    #[test]
    fn command_suggestions_complete_slash_prefix() {
        let s = command_suggestions("/mo");
        assert!(s.iter().any(|c| c == "/model"));
        // Needs a leading slash and no space yet.
        assert!(command_suggestions("model").is_empty());
        assert!(command_suggestions("/model foo").is_empty());
        // Exact full command offers no further suggestion.
        assert!(!command_suggestions("/he").is_empty());
    }

    #[test]
    fn fold_overwide_lines_splits_wide_cjk_lines_so_scroll_math_holds() {
        // A markdown line wider than the transcript must become multiple lines,
        // otherwise max_scroll undershoots and the bottom is unreachable.
        let wide = Line::from(Span::raw("한글".repeat(20))); // 80 columns
        let narrow = Line::from(Span::raw("ok"));
        let out = fold_overwide_lines(vec![wide, narrow], 30);
        assert_eq!(out.len(), 4); // ceil(80/30)=3 rows + 1
        for line in &out {
            let w: usize = line
                .spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(w <= 30);
        }
    }

    /// Rows of the transcript area from the drawn buffer, whitespace-stripped
    /// (wide-CJK continuation cells and padding disappear on both sides of
    /// the comparison).
    fn drawn_transcript_rows(
        terminal: &Terminal<ratatui::backend::TestBackend>,
        area: Rect,
    ) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..area.height)
            .map(|dy| {
                let mut row = String::new();
                for dx in 0..area.width {
                    row.push_str(buffer[(area.x + dx, area.y + dy)].symbol());
                }
                row.chars().filter(|c| !c.is_whitespace()).collect()
            })
            .collect()
    }

    /// Oracle: the OLD full-materialization pipeline (flatten everything,
    /// pad, slice) — the viewport render must show exactly these rows.
    fn expected_transcript_rows(
        app: &App,
        partial: Option<&str>,
        area: Rect,
        scroll: usize,
    ) -> Vec<String> {
        let width = area.width.max(1) as usize;
        let visible_rows = area.height as usize;
        let mut lines = transcript_lines(&app.entries, partial, width, app.tools_expanded);
        if app.follow && visible_rows > 0 && lines.len() < visible_rows {
            let pad = visible_rows - lines.len();
            let mut padded: Vec<Line<'static>> = Vec::with_capacity(visible_rows);
            padded.extend((0..pad).map(|_| Line::raw("")));
            padded.append(&mut lines);
            lines = padded;
        }
        (0..visible_rows)
            .map(|i| {
                lines
                    .get(scroll + i)
                    .map(|line| {
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>()
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    fn varied_entries(count: usize) -> Vec<Entry> {
        let mut entries = Vec::new();
        for i in 0..count {
            entries.push(Entry::new(
                Kind::User,
                format!("지시 {i} — {}", "한글명령 ".repeat(i % 9)),
            ));
            entries.push(Entry::new(
                Kind::Tool,
                format!(
                    "✓ bash — {} lines\n{}",
                    i % 5 + 1,
                    "output 한글출력\n".repeat(i % 5 + 1)
                ),
            ));
            entries.push(Entry::new(
                Kind::Assistant,
                format!(
                    "# 결과 {i}\n```rust\nfn f{i}() {{ let x = {i}; }}\n```\n- 항목 **강조** {}",
                    "본문내용 ".repeat(i % 13)
                ),
            ));
        }
        entries
    }

    #[test]
    fn viewport_render_matches_flattened_transcript_across_scrolls() {
        use ratatui::backend::TestBackend;
        let mut app = scroll_test_app();
        app.entries = varied_entries(60);
        let backend = TestBackend::new(84, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let partials: [Option<&str>; 2] = [
            None,
            Some("스트리밍 **중간** 응답\n```rust\nlet x = 1;\n```\n계속…"),
        ];
        for partial in partials {
            // Bottom-follow first, to learn the geometry and max scroll.
            app.follow = true;
            terminal.clear().unwrap();
            terminal.draw(|f| render(f, &mut app, partial)).unwrap();
            let area = app.last_transcript_area;
            assert!(area.height > 0 && area.width > 0);
            let max_scroll = app.last_max_scroll as usize;
            assert!(
                max_scroll > area.height as usize,
                "transcript too short for the test"
            );
            let follow_rows = drawn_transcript_rows(&terminal, area);
            assert_eq!(
                follow_rows,
                expected_transcript_rows(&app, partial, area, app.last_scroll as usize),
                "follow (partial={})",
                partial.is_some()
            );
            // Explicit scroll positions, including both boundaries and a
            // beyond-max value that must clamp.
            for scroll in [
                0usize,
                1,
                max_scroll / 2,
                max_scroll - 1,
                max_scroll,
                max_scroll + 99,
            ] {
                app.follow = false;
                app.scroll = scroll as u16;
                // TestBackend keeps stale symbols in cells hidden behind
                // wide CJK glyphs across frames — clear to isolate each one.
                terminal.clear().unwrap();
                terminal.draw(|f| render(f, &mut app, partial)).unwrap();
                let effective = app.last_scroll as usize;
                assert_eq!(effective, scroll.min(max_scroll), "clamping at {scroll}");
                assert_eq!(
                    drawn_transcript_rows(&terminal, area),
                    expected_transcript_rows(&app, partial, area, effective),
                    "scroll={scroll} (partial={})",
                    partial.is_some()
                );
            }
        }
    }

    #[test]
    fn viewport_render_survives_extreme_geometry() {
        use ratatui::backend::TestBackend;
        // No panics across degenerate terminal sizes, scroll extremes, empty
        // and huge transcripts, both tools_expanded states, selection active.
        let huge = Entry::new(Kind::Tool, "row 한글\n".repeat(5000));
        for (w, h) in [(1u16, 1u16), (3, 2), (20, 3), (250, 70)] {
            for empty in [true, false] {
                let mut app = scroll_test_app();
                if !empty {
                    app.entries = varied_entries(4);
                    app.entries.push(Entry::new(huge.kind, huge.text.clone()));
                }
                app.tools_expanded = w % 2 == 0;
                app.mouse_sel = Some((2, 5000));
                let backend = TestBackend::new(w, h);
                let mut terminal = Terminal::new(backend).unwrap();
                for (follow, scroll) in [(true, 0u16), (false, 0), (false, 1), (false, u16::MAX)] {
                    app.follow = follow;
                    app.scroll = scroll;
                    terminal.draw(|f| render(f, &mut app, None)).unwrap();
                    terminal
                        .draw(|f| render(f, &mut app, Some("부분 응답")))
                        .unwrap();
                }
            }
        }
    }

    #[test]
    fn viewport_render_warm_frames_stay_fast_on_long_transcripts() {
        use ratatui::backend::TestBackend;
        // 3000 entries ≈ a very long session. The first frame pays the cache
        // build (O(N), amortized); warm frames must stay bounded — before the
        // viewport rework every frame re-walked the entire transcript.
        let mut app = scroll_test_app();
        app.entries = (0..3000)
            .map(|i| Entry::new(Kind::Tool, format!("✓ bash #{i} — done\nline {i}")))
            .collect();
        let backend = TestBackend::new(100, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &mut app, None)).unwrap(); // cold build
        let started = std::time::Instant::now();
        let frames = 30;
        for i in 0..frames {
            app.follow = false;
            app.scroll = (i * 137) as u16; // wander around the transcript
            terminal.draw(|f| render(f, &mut app, None)).unwrap();
        }
        let avg = started.elapsed() / frames;
        eprintln!("warm frame avg: {avg:?} over {frames} frames, 3000 entries");
        assert!(
            avg < std::time::Duration::from_millis(50),
            "warm frames regressed to O(transcript): {avg:?}"
        );
    }

    #[test]
    fn entry_render_cache_reuses_lines_and_counts() {
        // The render path relies on: (1) counts matching the materialized
        // lines exactly (scroll math), (2) cache hits returning the SAME Rc
        // (no per-frame rebuild), (3) folded output (rows fit the width).
        let entry = Entry::new(
            Kind::Assistant,
            format!("# 제목\n{}", "한글본문 ".repeat(40)),
        );
        let count = entry_line_count(&entry, 30, false);
        let lines = entry_lines(&entry, 30, false);
        assert_eq!(count, lines.len());
        let again = entry_lines(&entry, 30, false);
        assert!(Rc::ptr_eq(&lines, &again));
        for line in lines.iter() {
            let w: usize = line
                .spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(w <= 30, "folded line wider than the transcript: {w}");
        }
        // A different width is a different layout — separate cache slot.
        let wider = entry_lines(&entry, 120, false);
        assert!(!Rc::ptr_eq(&lines, &wider));
    }

    #[test]
    fn brand_name_maps_known_domains() {
        assert_eq!(brand_name("raw.githubusercontent.com"), "GitHub");
        assert_eq!(brand_name("github.com"), "GitHub");
        assert_eq!(brand_name("platform.openai.com"), "OpenAI");
        assert_eq!(brand_name("claude.ai"), "Anthropic");
        assert_eq!(brand_name("docs.rs"), "Rust");
        // Unknown domains fall back to the host.
        assert_eq!(brand_name("example.org"), "example.org");
    }
}
