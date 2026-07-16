use anyhow::Result;

use crate::config::AppConfig;

pub struct KeybindingInfo {
    pub id: &'static str,
    pub keys: &'static str,
    pub description: &'static str,
}

pub fn default_keybindings() -> &'static [KeybindingInfo] {
    &[
        KeybindingInfo {
            id: "app.interrupt",
            keys: "escape",
            description: "Cancel or abort",
        },
        KeybindingInfo {
            id: "app.clear",
            keys: "ctrl+c",
            description: "Clear editor",
        },
        KeybindingInfo {
            id: "app.exit",
            keys: "ctrl+d",
            description: "Exit when editor is empty",
        },
        KeybindingInfo {
            id: "app.thinking.cycle",
            keys: "shift+tab",
            description: "Cycle thinking level",
        },
        KeybindingInfo {
            id: "app.model.cycleForward",
            keys: "ctrl+p",
            description: "Cycle to next favorite model",
        },
        KeybindingInfo {
            id: "app.model.cycleBackward",
            keys: "shift+ctrl+p",
            description: "Cycle to previous favorite model",
        },
        KeybindingInfo {
            id: "app.model.select",
            keys: "ctrl+l",
            description: "Open model selector",
        },
        KeybindingInfo {
            id: "app.tools.expand",
            keys: "ctrl+o",
            description: "Toggle tool output",
        },
        KeybindingInfo {
            id: "app.thinking.toggle",
            keys: "ctrl+t",
            description: "Toggle thinking blocks",
        },
        KeybindingInfo {
            id: "app.session.toggleNamedFilter",
            keys: "ctrl+n",
            description: "Toggle named session filter",
        },
        KeybindingInfo {
            id: "app.editor.external",
            keys: "ctrl+g",
            description: "Open external editor",
        },
        KeybindingInfo {
            id: "app.session.togglePath",
            keys: "ctrl+p",
            description: "Toggle session path display",
        },
        KeybindingInfo {
            id: "app.session.toggleSort",
            keys: "ctrl+s",
            description: "Toggle session sort mode",
        },
        KeybindingInfo {
            id: "app.session.rename",
            keys: "ctrl+r",
            description: "Rename session",
        },
        KeybindingInfo {
            id: "app.session.delete",
            keys: "ctrl+d",
            description: "Delete session",
        },
    ]
}

pub fn format_hotkeys_with_extensions(config: &AppConfig) -> Result<String> {
    let mut lines = default_keybindings()
        .iter()
        .map(|binding| format!("{}\t{}\t{}", binding.keys, binding.id, binding.description))
        .collect::<Vec<_>>();
    for shortcut in crate::extensions::load_extension_shortcuts(config)? {
        lines.push(format!(
            "{}\textension.{}.{}\t{}",
            shortcut.shortcut, shortcut.extension_id, shortcut.shortcut, shortcut.description
        ));
    }
    Ok(lines.join("\n"))
}
