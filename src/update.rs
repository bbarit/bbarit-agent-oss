//! Binary self-update: `bbarit-oss --upgrade`. Checks a version manifest, downloads
//! the matching prebuilt binary, and atomically replaces the running executable.
use anyhow::{Context, Result, anyhow, bail};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Where release artifacts live. Overridable for testing / self-hosting.
fn base_url() -> String {
    std::env::var("BBARIT_UPDATE_BASE").unwrap_or_else(|_| "https://bbarit.com/agent".to_string())
}

/// The platform key used in the release manifest and install script.
pub fn target_key() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x64",
        // No native arm64 Windows build yet; Windows-on-ARM runs the x64
        // binary under emulation, so upgrades (and installs) use that.
        ("windows", "aarch64") => "windows-x64",
        _ => return None,
    })
}

fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("bbarit-oss/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client")
}

/// Download the latest binary for this platform and replace the running one.
/// Fetch the latest published version + its download URL for this platform,
/// returning `Some((version, url))` only when it is strictly newer than the
/// running build. Any failure (offline, bad manifest, unknown platform) is a
/// quiet `None` — an update check must never interrupt normal use.
fn newer_release() -> Option<(String, String)> {
    let current = env!("CARGO_PKG_VERSION");
    let target = target_key()?;
    let manifest: serde_json::Value = client()
        .ok()?
        .get(format!("{}/latest.json", base_url()))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    let latest = manifest.get("version").and_then(|v| v.as_str())?;
    if latest == current {
        return None;
    }
    if let (Some(remote), Some(local)) = (parse_version(latest), parse_version(current))
        && remote <= local
    {
        return None;
    }
    let url = manifest
        .get("targets")
        .and_then(|t| t.get(target))
        .and_then(|u| u.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}/dist/{latest}/bbarit-oss-{target}{}",
                base_url(),
                exe_suffix()
            )
        });
    Some((latest.to_string(), url))
}

static AVAILABLE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn available_slot() -> &'static Mutex<Option<String>> {
    AVAILABLE.get_or_init(|| Mutex::new(None))
}

/// True unless the user opted out of the startup update check.
fn check_enabled() -> bool {
    let off = std::env::var("BBARIT_NO_UPDATE_CHECK")
        .ok()
        .or_else(|| crate::config::agent_env_var("BBARIT_NO_UPDATE_CHECK"));
    !matches!(off.as_deref().map(str::trim), Some("1") | Some("true"))
}

fn auto_upgrade_enabled() -> bool {
    let on = std::env::var("BBARIT_AUTO_UPGRADE")
        .ok()
        .or_else(|| crate::config::agent_env_var("BBARIT_AUTO_UPGRADE"));
    matches!(
        on.as_deref().map(str::trim),
        Some("1") | Some("true") | Some("on")
    )
}

/// Kick off a background check for a newer release. Non-blocking: startup never
/// waits on the network. The result is read later via [`available_update`].
pub fn spawn_startup_check() {
    if !check_enabled() || target_key().is_none() {
        return;
    }
    std::thread::spawn(|| {
        if let Some((version, _url)) = newer_release()
            && let Ok(mut slot) = available_slot().lock()
        {
            *slot = Some(version);
        }
    });
}

/// The newer version discovered by the startup check, if any (for a UI hint).
pub fn available_update() -> Option<String> {
    available_slot().lock().ok().and_then(|slot| slot.clone())
}

/// When `BBARIT_AUTO_UPGRADE` is on, upgrade in place at startup before the UI
/// opens. Blocking but bounded; failures are reported and otherwise ignored so
/// a bad network never blocks launch. Returns a one-line status when it acted.
pub fn maybe_auto_upgrade_at_startup() -> Option<String> {
    if !auto_upgrade_enabled() {
        return None;
    }
    let current = env!("CARGO_PKG_VERSION");
    let (version, url) = newer_release()?;
    let bytes = client()
        .ok()?
        .get(&url)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .ok()?;
    if bytes.len() < 1024 {
        return Some(format!(
            "auto-upgrade: skipped (download for v{version} looked invalid)"
        ));
    }
    let exe = std::env::current_exe().ok()?;
    match replace_executable(&exe, &bytes) {
        Ok(()) => Some(format!(
            "auto-upgraded v{current} → v{version} (restart to run it)"
        )),
        Err(err) => Some(format!("auto-upgrade to v{version} failed: {err}")),
    }
}

pub fn run() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let target = target_key().ok_or_else(|| {
        anyhow!(
            "no prebuilt binary for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    println!("Current version: {current}  ({target})");
    let manifest: serde_json::Value = client()?
        .get(format!("{}/latest.json", base_url()))
        .send()
        .context("failed to reach the update server")?
        .error_for_status()
        .context("update manifest request failed")?
        .json()
        .context("update manifest is not valid JSON")?;

    let latest = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("manifest has no version"))?;
    if latest == current {
        println!("Already up to date (v{current}).");
        return Ok(());
    }
    // Never downgrade: a stale mirror or rolled-back manifest must not replace
    // a newer local build. (Unparseable versions fall through and install.)
    if let (Some(remote), Some(local)) = (parse_version(latest), parse_version(current))
        && remote <= local
    {
        println!("Server offers v{latest}, which is not newer than v{current} — nothing to do.");
        return Ok(());
    }

    let url = manifest
        .get("targets")
        .and_then(|t| t.get(target))
        .and_then(|u| u.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}/dist/{latest}/bbarit-oss-{target}{}",
                base_url(),
                exe_suffix()
            )
        });

    println!("Downloading v{latest} …");
    let bytes = client()?
        .get(&url)
        .send()
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download failed: {url}"))?
        .bytes()
        .context("failed to read the downloaded binary")?;
    if bytes.len() < 1024 {
        bail!(
            "downloaded binary looks too small ({} bytes) — aborting",
            bytes.len()
        );
    }

    let exe = std::env::current_exe().context("cannot locate the running executable")?;
    replace_executable(&exe, &bytes)?;
    println!("Upgraded: v{current} → v{latest}");
    Ok(())
}

/// Parse "X.Y.Z" into a comparable triple; None when it isn't three numbers.
fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.trim().trim_start_matches('v').splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
}

/// Write `bytes` to a temp file beside `exe`, then swap it into place. On Unix a
/// rename over the running binary is safe; on Windows the running file is moved
/// aside first. The temp file never outlives a failure.
fn replace_executable(exe: &PathBuf, bytes: &[u8]) -> Result<()> {
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    let new_path = dir.join(format!("bbarit-oss-new-{}", std::process::id()));
    let result = write_and_swap(exe, &new_path, bytes);
    if result.is_err() {
        let _ = std::fs::remove_file(&new_path);
    }
    result
}

fn write_and_swap(exe: &PathBuf, new_path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    {
        let mut f = std::fs::File::create(new_path).with_context(|| {
            format!("cannot write to {} (try sudo, or reinstall)", dir.display())
        })?;
        f.write_all(bytes)?;
        f.flush()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new_path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(windows)]
    {
        // Can't overwrite a running .exe directly — move it aside first.
        let old = dir.join("bbarit-oss-old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).context("cannot move the current executable aside")?;
        if let Err(err) = std::fs::rename(new_path, exe) {
            let _ = std::fs::rename(&old, exe); // roll back
            return Err(err).context("cannot install the new executable");
        }
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::fs::rename(new_path, exe).context("cannot replace the current executable")?;
        Ok(())
    }
}
