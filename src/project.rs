//! Startup project picker: on an interactive launch, let the user choose the
//! codebase folder (a native folder dialog, or a numbered list of recently used
//! folders). The chosen folder becomes the process cwd before config load, and
//! is remembered in ~/.pi/agent/recent-projects.json for next time.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::Cli;

const MAX_RECENT: usize = 12;

/// Run the picker and switch the process cwd to the chosen folder, when an
/// interactive session is starting. No-op otherwise (print/json/rpc, piped
/// stdin/stdout, an explicit prompt/session, or `--no-pick`).
pub fn maybe_pick(cli: &Cli) -> Result<()> {
    if !should_pick(cli) {
        return Ok(());
    }
    if let Some(folder) = select_project()? {
        std::env::set_current_dir(&folder)
            .with_context(|| format!("failed to enter {}", folder.display()))?;
        remember(&folder);
    }
    Ok(())
}

fn should_pick(_cli: &Cli) -> bool {
    // BBARIT Terminal integration: the host terminal always launches bbarit in
    // the project's working directory, so the startup folder picker is redundant
    // and would pop a native dialog inside a terminal pane. Disabled at the
    // source so a launch always uses the inherited cwd.
    //
    // Reversible: the original interactive-launch heuristic was
    //     !cli.no_pick && cli.mode == OutputMode::Text && !cli.print
    //         && cli.inputs.is_empty() && cli.session.is_none() && cli.fork.is_none()
    //         && !cli.resume && !cli.continue_session
    //         && atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout)
    // Restore that body to re-enable the picker.
    false
}

fn agent_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::config::USER_APP_ROOT)
        .join("agent")
}

fn recent_path() -> PathBuf {
    agent_dir().join("recent-projects.json")
}

/// Recently opened project folders (most recent first), for the in-session
/// "change codebase" picker.
pub fn recent_projects() -> Vec<PathBuf> {
    load_recent()
}

/// Record `folder` as the most-recent project (used when switching mid-session).
pub fn remember_project(folder: &Path) {
    remember(folder);
}

fn load_recent() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(recent_path()) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(text.trim_start_matches('\u{feff}'))
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .collect()
}

fn save_recent(list: &[PathBuf]) {
    let path = recent_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let serializable: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
    if let Ok(text) = serde_json::to_string_pretty(&serializable) {
        let _ = std::fs::write(path, text);
    }
}

/// Move `folder` to the front of the recent list (deduped, capped).
fn remember(folder: &Path) {
    let mut list = load_recent();
    list.retain(|path| path != folder);
    list.insert(0, folder.to_path_buf());
    list.truncate(MAX_RECENT);
    save_recent(&list);
}

/// Show recent folders and a Browse option; return the chosen folder.
fn select_project() -> Result<Option<PathBuf>> {
    let recent = load_recent();
    let current = std::env::current_dir().ok();

    if recent.is_empty() {
        // First run: open the folder dialog; if it is cancelled or unavailable,
        // fall back to a text prompt so the launch never appears stuck.
        println!("\nbbarit — choose your project folder.");
        println!("A folder dialog will open (check the taskbar if it's behind this window)…");
        if let Some(folder) = pick_folder_dialog() {
            return Ok(Some(folder));
        }
        return Ok(prompt_path_or_current(current.as_deref()));
    }

    println!("\nbbarit — select a project folder:");
    for (index, path) in recent.iter().enumerate() {
        println!("  {}. {}", index + 1, path.display());
    }
    println!("  b. Browse for a folder…");
    if let Some(current) = &current {
        println!("  .  Use current folder ({})", current.display());
    }
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        return Ok(current);
    }
    let choice = input.trim();
    if choice.is_empty() || choice == "." {
        return Ok(current);
    }
    if choice.eq_ignore_ascii_case("b") {
        if let Some(folder) = pick_folder_dialog() {
            return Ok(Some(folder));
        }
        return Ok(prompt_path_or_current(current.as_deref()));
    }
    if let Ok(index) = choice.parse::<usize>()
        && index >= 1
        && index <= recent.len()
    {
        return Ok(Some(recent[index - 1].clone()));
    }
    // An unrecognized entry that is a real path is accepted directly.
    let path = PathBuf::from(choice);
    if path.is_dir() {
        return Ok(Some(path));
    }
    Ok(current)
}

/// Open a native folder-selection dialog. Windows uses the Shell folder browser
/// via PowerShell, owned by a TopMost form so it appears above the console;
/// other platforms return None (callers fall back to a text prompt).
fn pick_folder_dialog() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    // A hidden TopMost owner form forces the dialog to the foreground so it is
    // not lost behind the launcher console (which looked like "no response").
    let script = "Add-Type -AssemblyName System.Windows.Forms; \
         $owner = New-Object System.Windows.Forms.Form; \
         $owner.TopMost = $true; $owner.ShowInTaskbar = $false; \
         $owner.Size = New-Object System.Drawing.Size(1,1); \
         $owner.StartPosition = 'CenterScreen'; $owner.Show(); $owner.Activate(); \
         $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; \
         $dialog.Description = 'Select your project folder'; \
         $dialog.ShowNewFolderButton = $true; \
         $result = $dialog.ShowDialog($owner); $owner.Close(); \
         if ($result -eq [System.Windows.Forms.DialogResult]::OK) { \
             [Console]::Out.Write($dialog.SelectedPath) }";
    let output = crate::spawn::no_window_command("powershell")
        .args(["-NoProfile", "-STA", "-Command", script])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    let path = PathBuf::from(path);
    path.is_dir().then_some(path)
}

/// Text fallback when the GUI dialog is cancelled or unavailable: ask for a
/// folder path, defaulting to the current directory on an empty line.
fn prompt_path_or_current(current: Option<&Path>) -> Option<PathBuf> {
    match current {
        Some(current) => print!("Project folder path (Enter = {}): ", current.display()),
        None => print!("Project folder path: "),
    }
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return current.map(Path::to_path_buf);
    }
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return current.map(Path::to_path_buf);
    }
    let path = PathBuf::from(trimmed);
    if path.is_dir() {
        Some(path)
    } else {
        current.map(Path::to_path_buf)
    }
}
