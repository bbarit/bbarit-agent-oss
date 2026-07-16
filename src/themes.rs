// Color resolution helpers (colorize/active_theme/fg_prefix and the parsed
// color tables) are retained for theme loading and the /themes RPC even though
// the ratatui TUI now styles output directly.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub file_path: PathBuf,
    colors: BTreeMap<String, AnsiColor>,
}

#[derive(Debug, Deserialize)]
struct ThemeJson {
    name: Option<String>,
    #[serde(default)]
    vars: BTreeMap<String, Value>,
    #[serde(default)]
    colors: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
enum AnsiColor {
    TrueColor(u8, u8, u8),
    Indexed(u8),
    Default,
}

impl AnsiColor {
    fn fg_prefix(&self) -> String {
        match self {
            Self::TrueColor(r, g, b) if !truecolor_supported() => {
                format!("\x1b[38;5;{}m", rgb_to_indexed(*r, *g, *b))
            }
            Self::TrueColor(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
            Self::Indexed(index) => format!("\x1b[38;5;{index}m"),
            Self::Default => String::new(),
        }
    }
}

/// Terminals without truecolor (e.g. Terminal.app) ignore 24-bit SGR (38;2;…) and
/// draw with the default foreground — why the whole screen looked white. For those terminals
/// we down-map to the nearest xterm-256 index instead of dropping color.
pub fn truecolor_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let colorterm = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return true;
        }
        if std::env::var("TERM").unwrap_or_default().contains("direct") {
            return true;
        }
        // COLORTERM unset + Apple Terminal is the definite unsupported combo. Otherwise
        // keep the existing truecolor behavior to preserve color accuracy on capable terminals.
        std::env::var("TERM_PROGRAM").as_deref() != Ok("Apple_Terminal")
    })
}

/// Nearest xterm-256 index: grays use the 232–255 ramp, everything else the 6×6×6 color cube.
pub fn rgb_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max - min < 12 {
        let v = (r as u16 + g as u16 + b as u16) / 3;
        if v < 5 {
            return 16; // cube black
        }
        if v > 246 {
            return 231; // cube white
        }
        return 232 + ((v - 3) / 10).min(23) as u8;
    }
    let scale = |v: u8| -> u16 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v as u16 - 35) / 40
        }
    };
    (16 + 36 * scale(r) + 6 * scale(g) + scale(b)) as u8
}

pub fn load_themes(config: &AppConfig) -> Result<Vec<Theme>> {
    let mut themes = Vec::new();
    if !config.no_themes {
        for dir in default_theme_dirs(config) {
            load_themes_from_path(&dir, &mut themes)?;
        }
        for dir in crate::extensions::resource_dirs(config, "themes")? {
            load_themes_from_path(&dir, &mut themes)?;
        }
    }
    for path in &config.theme_paths {
        load_themes_from_path(path, &mut themes)?;
    }

    let mut deduped = BTreeMap::new();
    for theme in themes {
        deduped.entry(theme.name.clone()).or_insert(theme);
    }
    Ok(deduped.into_values().collect())
}

pub fn format_theme_list(config: &AppConfig) -> Result<String> {
    let themes = load_themes(config)?;
    if themes.is_empty() {
        return Ok("No themes found.".to_string());
    }
    Ok(themes
        .into_iter()
        .map(|theme| format!("{}\t{}", theme.name, theme.file_path.display()))
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn format_theme_detail(config: &AppConfig, name: &str) -> Result<String> {
    let theme = load_themes(config)?
        .into_iter()
        .find(|theme| theme.name == name)
        .ok_or_else(|| anyhow::anyhow!("no theme named {name}"))?;
    let raw = fs::read_to_string(&theme.file_path)
        .with_context(|| format!("failed to read theme {}", theme.file_path.display()))?;
    Ok(format!(
        "{}\nPath: {}\n\n{}",
        theme.name,
        theme.file_path.display(),
        raw.trim()
    ))
}

pub fn active_theme(config: &AppConfig) -> Option<Theme> {
    if config.no_themes {
        return None;
    }
    for path in &config.theme_paths {
        let mut themes = Vec::new();
        if load_themes_from_path(path, &mut themes).is_ok()
            && let Some(theme) = themes.into_iter().next()
        {
            return Some(theme);
        }
    }
    load_themes(config)
        .ok()
        .and_then(|themes| themes.into_iter().next())
}

pub fn colorize(theme: Option<&Theme>, key: &str, text: &str) -> String {
    let Some(theme) = theme else {
        return text.to_string();
    };
    let Some(color) = theme.colors.get(key) else {
        return text.to_string();
    };
    let prefix = color.fg_prefix();
    if prefix.is_empty() {
        text.to_string()
    } else {
        format!("{prefix}{text}\x1b[0m")
    }
}

fn default_theme_dirs(config: &AppConfig) -> Vec<PathBuf> {
    let mut paths = vec![config.user_app_dir.join("themes")];
    if config.project_trusted {
        paths.push(config.app_dir.join("themes"));
    }
    paths
}

fn load_themes_from_path(path: &Path, out: &mut Vec<Theme>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            out.push(read_theme(path)?);
        }
        return Ok(());
    }
    if path.is_dir() {
        for entry in
            fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))?
        {
            let path = entry?.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                out.push(read_theme(&path)?);
            }
        }
    }
    Ok(())
}

fn read_theme(path: &Path) -> Result<Theme> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read theme {}", path.display()))?;
    let parsed: ThemeJson = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse theme {}", path.display()))?;
    let colors = resolve_colors(&parsed);
    let name = parsed
        .name
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToOwned::to_owned)
        })
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("theme {} has no name", path.display()))?;
    if name.contains('\t') || name.contains('\n') || name.contains('\r') {
        bail!("theme {} has an invalid name", path.display());
    }
    Ok(Theme {
        name,
        file_path: path.to_path_buf(),
        colors,
    })
}

fn resolve_colors(theme: &ThemeJson) -> BTreeMap<String, AnsiColor> {
    theme
        .colors
        .iter()
        .filter_map(|(key, value)| {
            resolve_color(value, &theme.vars).map(|color| (key.clone(), color))
        })
        .collect()
}

fn resolve_color(value: &Value, vars: &BTreeMap<String, Value>) -> Option<AnsiColor> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .map(AnsiColor::Indexed),
        Value::String(value) => resolve_color_string(value, vars, 0),
        _ => None,
    }
}

fn resolve_color_string(
    value: &str,
    vars: &BTreeMap<String, Value>,
    depth: usize,
) -> Option<AnsiColor> {
    if value.trim().is_empty() {
        return Some(AnsiColor::Default);
    }
    if depth > 8 {
        return None;
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(var) = vars.get(value) {
        return match var {
            Value::String(next) => resolve_color_string(next, vars, depth + 1),
            Value::Number(number) => number
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
                .map(AnsiColor::Indexed),
            _ => None,
        };
    }
    named_color(value)
}

fn parse_hex_color(hex: &str) -> Option<AnsiColor> {
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(AnsiColor::TrueColor(r, g, b))
}

fn named_color(value: &str) -> Option<AnsiColor> {
    let index = match value.to_ascii_lowercase().as_str() {
        "black" => 0,
        "red" => 1,
        "green" => 2,
        "yellow" => 3,
        "blue" => 4,
        "magenta" => 5,
        "cyan" => 6,
        "white" => 7,
        "gray" | "grey" | "mediumgray" | "mediumgrey" => 8,
        "darkgray" | "darkgrey" | "dimgray" | "dimgrey" => 240,
        _ => return None,
    };
    Some(AnsiColor::Indexed(index))
}

#[cfg(test)]
mod truecolor_tests {
    use super::rgb_to_indexed;

    #[test]
    fn should_map_brand_palette_into_256_cube() {
        // Check the color-cube coordinates stay in the right family — BRAND_RED should land in the cell
        // with the largest red component, and ACCENT in a blue/cyan cell.
        let red = rgb_to_indexed(0xE0, 0x52, 0x52);
        let (r, g, b) = cube(red);
        assert!(r > g && r > b, "brand red mapped to {red} ({r},{g},{b})");

        let accent = rgb_to_indexed(0x4F, 0xC5, 0xE0);
        let (r, g, b) = cube(accent);
        assert!(b >= g && g > r, "accent mapped to {accent} ({r},{g},{b})");
    }

    #[test]
    fn should_map_grays_to_gray_ramp() {
        assert_eq!(rgb_to_indexed(0, 0, 0), 16);
        assert_eq!(rgb_to_indexed(255, 255, 255), 231);
        let mid = rgb_to_indexed(0x6a, 0x6a, 0x6a); // CARD_FRAME
        assert!((232..=255).contains(&mid), "mid gray mapped to {mid}");
    }

    fn cube(index: u8) -> (u8, u8, u8) {
        let v = index - 16;
        (v / 36, (v % 36) / 6, v % 6)
    }
}
