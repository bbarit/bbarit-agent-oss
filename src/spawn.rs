//! Windows-safe process spawning for the agent.
//!
//! When embedded in a GUI-subsystem host binary,
//! the parent has no console: every `std::process::Command` spawn of a console
//! program (git, cmd, node, powershell, …) makes Windows allocate a fresh conhost
//! window that flashes on screen. CREATE_NO_WINDOW gives the child an invisible
//! console instead. Inherited/piped stdio handles are unaffected, so captured
//! output and TUI-mode (console parent) spawns keep working. No-op elsewhere.

/// Drop-in replacement for `std::process::Command::new` that never pops up a
/// console window on Windows.
pub fn no_window_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// `Command` for the Node.js runtime. A GUI-launched app on macOS inherits
/// launchd's minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`) which has no node,
/// so a bare `Command::new("node")` in the installed .app fails with ENOENT
/// even though node is installed. When PATH lookup would miss, resolve a
/// concrete binary from well-known install locations and prepend its directory
/// to the child's PATH so nested node spawns keep working too.
pub fn node_command() -> std::process::Command {
    let node = resolved_node();
    let mut cmd = no_window_command(node);
    if let Some(dir) = std::path::Path::new(node)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        let existing = std::env::var("PATH").unwrap_or_default();
        if !std::env::split_paths(&existing).any(|p| p == dir) {
            let sep = if cfg!(windows) { ';' } else { ':' };
            cmd.env("PATH", format!("{}{sep}{existing}", dir.display()));
        }
    }
    cmd
}

fn resolved_node() -> &'static str {
    static NODE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NODE.get_or_init(|| {
        let exe = if cfg!(windows) { "node.exe" } else { "node" };
        // PATH first — identical behavior to `Command::new("node")` whenever it works.
        if let Some(paths) = std::env::var_os("PATH")
            && std::env::split_paths(&paths)
                .any(|dir| !dir.as_os_str().is_empty() && dir.join(exe).is_file())
        {
            return "node".to_string();
        }
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let mut candidates: Vec<std::path::PathBuf> = if cfg!(windows) {
            vec![
                r"C:\Program Files\nodejs\node.exe".into(),
                format!(r"{home}\scoop\shims\node.exe").into(),
                format!(r"{home}\AppData\Local\Programs\nodejs\node.exe").into(),
            ]
        } else {
            vec![
                "/opt/homebrew/bin/node".into(),
                "/usr/local/bin/node".into(),
                format!("{home}/.volta/bin/node").into(),
                format!("{home}/.local/bin/node").into(),
            ]
        };
        if let Some(nvm) = latest_nvm_node(&home) {
            candidates.push(nvm);
        }
        candidates
            .into_iter()
            .find(|p| p.is_file())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "node".to_string())
    })
}

/// Highest-version `~/.nvm/versions/node/v*/bin/node`, compared numerically
/// (lexicographic max would pick v9 over v10).
fn latest_nvm_node(home: &str) -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(home)
        .join(".nvm")
        .join("versions")
        .join("node");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let version = parse_node_version(&name)?;
            let node = entry.path().join("bin").join("node");
            node.is_file().then_some((version, node))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, node)| node)
}

fn parse_node_version(name: &str) -> Option<(u64, u64, u64)> {
    let mut parts = name.strip_prefix('v')?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::parse_node_version;

    #[test]
    fn parses_full_and_partial_versions() {
        assert_eq!(parse_node_version("v24.14.0"), Some((24, 14, 0)));
        assert_eq!(parse_node_version("v20"), Some((20, 0, 0)));
        assert_eq!(parse_node_version("node"), None);
        assert_eq!(parse_node_version("v20.x"), None);
    }

    #[test]
    fn version_tuples_compare_numerically_not_lexicographically() {
        assert!(parse_node_version("v10.0.0") > parse_node_version("v9.11.2"));
    }
}
