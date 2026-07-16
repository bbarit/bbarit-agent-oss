//! Computer use — a tool that sees the whole desktop (screenshot) and controls it (mouse/keyboard).
//! Screenshots are rescaled to the logical resolution and saved; the coordinates the model gives
//! are in "screenshot pixels" → converted to screen points on click. Images are attached to the
//! next user message via ATTACH_IMAGE_MARKER and passed to the model.
//!
//! On macOS, without Accessibility permission CGEvent is silently
//! dropped — if clicks/typing do nothing, we return a permissions hint as an error.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::config::AppConfig;

/// When a tool result carries this marker, the agent loop strips it and attaches the image
/// to the next user message (tool results themselves are text-only).
pub const ATTACH_IMAGE_MARKER: &str = "[[bbarit-attach-image:";
pub const ATTACH_IMAGE_MARKER_END: &str = "]]";

/// Max screenshot width (px) — a trade-off between vision-token cost and coordinate precision.
const MAX_SCREENSHOT_WIDTH: u32 = 1456;

/// Scale (image px → screen points) of the last screenshot. 1.0 when clicking without a screenshot.
static LAST_SCALE: Mutex<(f64, f64)> = Mutex::new((1.0, 1.0));

pub fn computer_action_is_readonly(action: &str) -> bool {
    matches!(action, "screenshot" | "")
}

/// Computer-use enable toggle variable (the agent dotenv or process env).
pub const COMPUTER_USE_ENV: &str = "BBARIT_COMPUTER_USE";

/// Opt-in: the computer tool is exposed to the model only after /computer on. Because it controls
/// the whole desktop, it is off by default.
pub fn computer_use_enabled() -> bool {
    crate::config::agent_env_var(COMPUTER_USE_ENV)
        .and_then(|value| parse_toggle(&value))
        .unwrap_or(false)
}

/// Parse an "on"/"off"-style toggle string. Pure function — under test.
pub fn parse_toggle(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" | "enable" | "enabled" => Some(true),
        "0" | "off" | "false" | "no" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

/// Persist the toggle to the agent dotenv (1 when on, remove the line when off).
pub fn set_computer_use_enabled(enabled: bool) -> Result<()> {
    crate::config::set_agent_env_var(COMPUTER_USE_ENV, enabled.then_some("1"))
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn int_arg(args: &Value, key: &str) -> Option<i32> {
    args.get(key)
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .or_else(|| {
            args.get(key)
                .and_then(Value::as_f64)
                .map(|v| v.round() as i32)
        })
}

fn enigo() -> Result<enigo::Enigo> {
    enigo::Enigo::new(&enigo::Settings::default()).map_err(|e| {
        anyhow!(
            "cannot initialize input control: {e}{}",
            accessibility_hint()
        )
    })
}

fn accessibility_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        " — on macOS grant Accessibility permission to the host app (System Settings → \
         Privacy & Security → Accessibility → enable BBARIT Terminal / your terminal), \
         then retry"
    } else {
        ""
    }
}

/// Model coordinates (screenshot px) → screen points.
fn to_screen(x: i32, y: i32) -> (i32, i32) {
    let (sx, sy) = LAST_SCALE.lock().map(|g| *g).unwrap_or((1.0, 1.0));
    (
        (f64::from(x) * sx).round() as i32,
        (f64::from(y) * sy).round() as i32,
    )
}

pub fn execute(config: &AppConfig, args: &Value) -> Result<String> {
    if !computer_use_enabled() {
        bail!("computer use is disabled — the user can enable it with /computer on");
    }
    let action = str_arg(args, "action")
        .ok_or_else(|| anyhow!(
            "computer: 'action' is required (screenshot|click|double_click|right_click|move|drag|type|key|scroll)"
        ))?;
    match action {
        "screenshot" => screenshot(config),
        "click" | "double_click" | "right_click" | "middle_click" => click(args, action),
        "move" => mouse_move(args),
        "drag" => drag(args),
        "type" => type_text(args),
        "key" => key_combo(args),
        "scroll" => scroll(args),
        other => bail!("computer: unknown action '{other}'"),
    }
}

fn screenshot_path(config: &AppConfig) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    config
        .cwd
        .join("computer-use")
        .join(format!("screen-{stamp}.png"))
}

#[cfg(target_os = "macos")]
fn capture_screen(path: &std::path::Path) -> Result<()> {
    // -x silent, -m main display only (the coordinate system is main-relative on multi-monitor).
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-m"])
        .arg(path)
        .status()?;
    if !status.success() {
        bail!(
            "screencapture failed ({status}) — grant Screen Recording permission to the host app (System Settings → Privacy & Security → Screen Recording)"
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn capture_screen(path: &std::path::Path) -> Result<()> {
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \
         $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; \
         $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; \
         $g=[System.Drawing.Graphics]::FromImage($bmp); \
         $g.CopyFromScreen($b.X,$b.Y,0,0,$bmp.Size); \
         $bmp.Save('{}',[System.Drawing.Imaging.ImageFormat]::Png)",
        path.display()
    );
    let status = crate::spawn::no_window_command("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()?;
    if !status.success() {
        bail!("powershell screenshot failed ({status})");
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn capture_screen(_path: &std::path::Path) -> Result<()> {
    bail!("computer screenshot is not supported on this platform yet")
}

#[cfg(target_os = "macos")]
fn image_pixel_size(path: &std::path::Path) -> Result<(u32, u32)> {
    let output = std::process::Command::new("sips")
        .args(["-g", "pixelWidth", "-g", "pixelHeight"])
        .arg(path)
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let get = |key: &str| -> Option<u32> {
        text.lines()
            .find(|line| line.contains(key))
            .and_then(|line| line.split(':').nth(1))
            .and_then(|v| v.trim().parse().ok())
    };
    match (get("pixelWidth"), get("pixelHeight")) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => bail!("cannot read screenshot dimensions"),
    }
}

#[cfg(target_os = "macos")]
fn resize_image_width(path: &std::path::Path, width: u32) -> Result<()> {
    let status = std::process::Command::new("sips")
        .args(["--resampleWidth", &width.to_string()])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("sips resize failed ({status})");
    }
    Ok(())
}

/// Logical size (points) of the main display. On macOS we read it directly via CoreGraphics,
/// which needs no permission — initializing enigo would require Accessibility and block even
/// screenshots (observation).
fn main_display_size() -> Result<(i32, i32)> {
    #[cfg(target_os = "macos")]
    {
        let bounds = core_graphics::display::CGDisplay::main().bounds();
        Ok((bounds.size.width as i32, bounds.size.height as i32))
    }
    #[cfg(not(target_os = "macos"))]
    {
        use enigo::Mouse as _;
        let e = enigo()?;
        e.main_display().map_err(|err| anyhow!("{err}"))
    }
}

fn screenshot(config: &AppConfig) -> Result<String> {
    let path = screenshot_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    capture_screen(&path)?;

    // Screen logical size (mouse coordinate system).
    let (logical_w, logical_h) = main_display_size()?;

    #[cfg(target_os = "macos")]
    {
        // Resample Retina (2x) pixels → the target width. The coordinate system is unified on
        // screenshot px; we remember only the scale to convert to points on click.
        let target_w = u32::try_from(logical_w.max(1))
            .unwrap_or(MAX_SCREENSHOT_WIDTH)
            .min(MAX_SCREENSHOT_WIDTH);
        resize_image_width(&path, target_w)?;
        let (img_w, img_h) = image_pixel_size(&path)?;
        if let Ok(mut guard) = LAST_SCALE.lock() {
            *guard = (
                f64::from(logical_w) / f64::from(img_w.max(1)),
                f64::from(logical_h) / f64::from(img_h.max(1)),
            );
        }
        Ok(format!(
            "Screenshot of the main display ({logical_w}x{logical_h} points) saved to {} \
             as {img_w}x{img_h}px.\nGive all computer coordinates in SCREENSHOT pixels \
             (0,0 = top-left) — they are mapped to the screen automatically.\n\
             {ATTACH_IMAGE_MARKER}{}{ATTACH_IMAGE_MARKER_END}",
            path.display(),
            path.display(),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(mut guard) = LAST_SCALE.lock() {
            *guard = (1.0, 1.0);
        }
        Ok(format!(
            "Screenshot of the screen ({logical_w}x{logical_h}) saved to {}.\nGive all \
             computer coordinates in screenshot pixels (0,0 = top-left).\n\
             {ATTACH_IMAGE_MARKER}{}{ATTACH_IMAGE_MARKER_END}",
            path.display(),
            path.display(),
        ))
    }
}

fn require_xy(args: &Value) -> Result<(i32, i32)> {
    match (int_arg(args, "x"), int_arg(args, "y")) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => bail!("computer: 'x' and 'y' (screenshot pixels) are required"),
    }
}

fn click(args: &Value, action: &str) -> Result<String> {
    use enigo::{Button, Direction, Mouse as _};
    let (x, y) = require_xy(args)?;
    let (sx, sy) = to_screen(x, y);
    let mut e = enigo()?;
    e.move_mouse(sx, sy, enigo::Coordinate::Abs)
        .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    std::thread::sleep(Duration::from_millis(60));
    let button = match action {
        "right_click" => Button::Right,
        "middle_click" => Button::Middle,
        _ => Button::Left,
    };
    let clicks = if action == "double_click" { 2 } else { 1 };
    for i in 0..clicks {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(80));
        }
        e.button(button, Direction::Click)
            .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    }
    Ok(format!(
        "{action} at ({x}, {y}) [screen point ({sx}, {sy})]. Take a screenshot to verify the result."
    ))
}

fn mouse_move(args: &Value) -> Result<String> {
    use enigo::Mouse as _;
    let (x, y) = require_xy(args)?;
    let (sx, sy) = to_screen(x, y);
    enigo()?
        .move_mouse(sx, sy, enigo::Coordinate::Abs)
        .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    Ok(format!("Moved the mouse to ({x}, {y})."))
}

fn drag(args: &Value) -> Result<String> {
    use enigo::{Button, Direction, Mouse as _};
    let (x, y) = require_xy(args)?;
    let (x2, y2) = match (int_arg(args, "x2"), int_arg(args, "y2")) {
        (Some(x2), Some(y2)) => (x2, y2),
        _ => bail!("computer drag: 'x2' and 'y2' are required"),
    };
    let (sx, sy) = to_screen(x, y);
    let (sx2, sy2) = to_screen(x2, y2);
    let mut e = enigo()?;
    e.move_mouse(sx, sy, enigo::Coordinate::Abs)
        .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    std::thread::sleep(Duration::from_millis(80));
    e.button(Button::Left, Direction::Press)
        .map_err(|err| anyhow!("{err}"))?;
    // Many apps only recognize a drag if it moves through an intermediate point.
    let steps = 8;
    for i in 1..=steps {
        let ix = sx + (sx2 - sx) * i / steps;
        let iy = sy + (sy2 - sy) * i / steps;
        e.move_mouse(ix, iy, enigo::Coordinate::Abs)
            .map_err(|err| anyhow!("{err}"))?;
        std::thread::sleep(Duration::from_millis(25));
    }
    e.button(Button::Left, Direction::Release)
        .map_err(|err| anyhow!("{err}"))?;
    Ok(format!("Dragged from ({x}, {y}) to ({x2}, {y2})."))
}

fn type_text(args: &Value) -> Result<String> {
    use enigo::Keyboard as _;
    let text = str_arg(args, "text").ok_or_else(|| anyhow!("computer type: 'text' is required"))?;
    enigo()?
        .text(text)
        .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    Ok(format!("Typed {} character(s).", text.chars().count()))
}

/// Parse combos like "cmd+shift+t", "enter", "ctrl+c". Pure function — under test.
pub fn parse_key_combo(combo: &str) -> Result<(Vec<enigo::Key>, enigo::Key)> {
    use enigo::Key;
    let parts: Vec<String> = combo
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        bail!("computer key: 'key' is required (e.g. \"enter\", \"cmd+c\", \"ctrl+shift+t\")");
    }
    let mut modifiers = Vec::new();
    let mut main: Option<Key> = None;
    let count = parts.len();
    for (index, part) in parts.iter().enumerate() {
        let is_last = index + 1 == count;
        let as_modifier = match part.as_str() {
            "cmd" | "command" | "meta" | "super" | "win" => Some(Key::Meta),
            "ctrl" | "control" => Some(Key::Control),
            "alt" | "option" | "opt" => Some(Key::Alt),
            "shift" => Some(Key::Shift),
            _ => None,
        };
        if let Some(modifier) = as_modifier
            && !is_last
        {
            modifiers.push(modifier);
            continue;
        }
        let key = match part.as_str() {
            "enter" | "return" => Key::Return,
            "tab" => Key::Tab,
            "esc" | "escape" => Key::Escape,
            "space" => Key::Space,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "up" => Key::UpArrow,
            "down" => Key::DownArrow,
            "left" => Key::LeftArrow,
            "right" => Key::RightArrow,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "f1" => Key::F1,
            "f2" => Key::F2,
            "f3" => Key::F3,
            "f4" => Key::F4,
            "f5" => Key::F5,
            "f6" => Key::F6,
            "f7" => Key::F7,
            "f8" => Key::F8,
            "f9" => Key::F9,
            "f10" => Key::F10,
            "f11" => Key::F11,
            "f12" => Key::F12,
            "cmd" | "command" | "meta" | "super" | "win" => Key::Meta,
            "ctrl" | "control" => Key::Control,
            "alt" | "option" | "opt" => Key::Alt,
            "shift" => Key::Shift,
            single if single.chars().count() == 1 => Key::Unicode(single.chars().next().unwrap()),
            other => bail!("computer key: unknown key '{other}'"),
        };
        main = Some(key);
    }
    let main = main.ok_or_else(|| anyhow!("computer key: no main key in '{combo}'"))?;
    Ok((modifiers, main))
}

fn key_combo(args: &Value) -> Result<String> {
    use enigo::{Direction, Keyboard as _};
    let combo = str_arg(args, "key").ok_or_else(|| {
        anyhow!("computer key: 'key' is required (e.g. \"enter\", \"cmd+c\", \"ctrl+shift+t\")")
    })?;
    let (modifiers, main) = parse_key_combo(combo)?;
    let mut e = enigo()?;
    for modifier in &modifiers {
        e.key(*modifier, Direction::Press)
            .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    }
    let result = e.key(main, Direction::Click);
    for modifier in modifiers.iter().rev() {
        let _ = e.key(*modifier, Direction::Release);
    }
    result.map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    Ok(format!("Pressed {combo}."))
}

fn scroll(args: &Value) -> Result<String> {
    use enigo::{Axis, Mouse as _};
    let mut e = enigo()?;
    if let (Some(x), Some(y)) = (int_arg(args, "x"), int_arg(args, "y")) {
        let (sx, sy) = to_screen(x, y);
        e.move_mouse(sx, sy, enigo::Coordinate::Abs)
            .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
        std::thread::sleep(Duration::from_millis(50));
    }
    let amount = int_arg(args, "amount").unwrap_or(3).clamp(1, 30);
    let direction = str_arg(args, "direction").unwrap_or("down");
    let (axis, delta) = match direction {
        "up" => (Axis::Vertical, -amount),
        "down" => (Axis::Vertical, amount),
        "left" => (Axis::Horizontal, -amount),
        "right" => (Axis::Horizontal, amount),
        other => bail!("computer scroll: direction must be up|down|left|right (got '{other}')"),
    };
    e.scroll(delta, axis)
        .map_err(|err| anyhow!("{err}{}", accessibility_hint()))?;
    Ok(format!("Scrolled {direction} by {amount}."))
}

/// Strip attachment markers from a tool result and return (cleaned text, image paths).
/// Pure function — used by the agent loop.
pub fn extract_image_attachments(result: &str) -> (String, Vec<String>) {
    let mut cleaned = String::with_capacity(result.len());
    let mut paths = Vec::new();
    let mut rest = result;
    while let Some(start) = rest.find(ATTACH_IMAGE_MARKER) {
        cleaned.push_str(&rest[..start]);
        let after = &rest[start + ATTACH_IMAGE_MARKER.len()..];
        match after.find(ATTACH_IMAGE_MARKER_END) {
            Some(end) => {
                let path = after[..end].trim();
                if !path.is_empty() {
                    paths.push(path.to_string());
                }
                rest = &after[end + ATTACH_IMAGE_MARKER_END.len()..];
            }
            None => {
                rest = after;
                break;
            }
        }
    }
    cleaned.push_str(rest);
    (cleaned.trim_end().to_string(), paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_image_attachments_strips_markers_and_collects_paths() {
        let input = format!(
            "Screenshot saved.\n{ATTACH_IMAGE_MARKER}/tmp/a.png{ATTACH_IMAGE_MARKER_END}\nmore\n{ATTACH_IMAGE_MARKER}/tmp/b.png{ATTACH_IMAGE_MARKER_END}"
        );
        let (cleaned, paths) = extract_image_attachments(&input);
        assert_eq!(paths, vec!["/tmp/a.png", "/tmp/b.png"]);
        assert!(!cleaned.contains("bbarit-attach-image"));
        assert!(cleaned.contains("Screenshot saved."));
        assert!(cleaned.contains("more"));
    }

    #[test]
    fn extract_image_attachments_passes_plain_text_through() {
        let (cleaned, paths) = extract_image_attachments("no markers here");
        assert_eq!(cleaned, "no markers here");
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_key_combo_handles_modifiers_and_aliases() {
        use enigo::Key;
        let (mods, main) = parse_key_combo("cmd+shift+t").unwrap();
        assert_eq!(mods, vec![Key::Meta, Key::Shift]);
        assert_eq!(main, Key::Unicode('t'));
        let (mods, main) = parse_key_combo("enter").unwrap();
        assert!(mods.is_empty());
        assert_eq!(main, Key::Return);
        // Allow modifier keys on their own (e.g. "cmd").
        let (mods, main) = parse_key_combo("cmd").unwrap();
        assert!(mods.is_empty());
        assert_eq!(main, Key::Meta);
        assert!(parse_key_combo("cmd+definitely-not-a-key").is_err());
    }

    #[test]
    fn computer_readonly_actions() {
        assert!(computer_action_is_readonly("screenshot"));
        assert!(!computer_action_is_readonly("click"));
        assert!(!computer_action_is_readonly("type"));
    }

    #[test]
    fn parse_toggle_accepts_common_forms() {
        assert_eq!(parse_toggle("on"), Some(true));
        assert_eq!(parse_toggle("1"), Some(true));
        assert_eq!(parse_toggle("TRUE"), Some(true));
        assert_eq!(parse_toggle("off"), Some(false));
        assert_eq!(parse_toggle("0"), Some(false));
        assert_eq!(parse_toggle("maybe"), None);
    }
}
