use std::ffi::OsString;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputMode {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "bbarit-oss",
    version,
    about = "bbarit agent — a fast Rust coding agent"
)]
pub struct Cli {
    // Default priority: Codex (ChatGPT subscription) first — see config.rs
    // provider resolution, which treats this default as "not explicitly set".
    #[arg(long, default_value = "openai-codex")]
    pub provider: String,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long)]
    pub thinking: Option<String>,

    /// Adopt a specialist persona at startup (id, name, or search term —
    /// see /persona for the library).
    #[arg(long)]
    pub persona: Option<String>,

    #[arg(long)]
    pub api_key: Option<String>,

    #[arg(long)]
    pub system_prompt: Option<String>,

    #[arg(long)]
    pub append_system_prompt: Vec<String>,

    #[arg(long = "models")]
    pub favorite_models: Option<String>,

    #[arg(long = "no-tools")]
    pub no_tools: bool,

    #[arg(long = "no-builtin-tools")]
    pub no_builtin_tools: bool,

    #[arg(long = "tools", short = 't')]
    pub tools: Option<String>,

    #[arg(long = "exclude-tools")]
    pub exclude_tools: Option<String>,

    #[arg(long = "extension", short = 'e')]
    pub extensions: Vec<PathBuf>,

    #[arg(long = "no-extensions")]
    pub no_extensions: bool,

    #[arg(long = "skill")]
    pub skills: Vec<PathBuf>,

    #[arg(long = "no-skills")]
    pub no_skills: bool,

    #[arg(long = "prompt-template")]
    pub prompt_templates: Vec<PathBuf>,

    #[arg(long = "no-prompt-templates")]
    pub no_prompt_templates: bool,

    #[arg(long = "theme")]
    pub themes: Vec<PathBuf>,

    #[arg(long = "no-themes")]
    pub no_themes: bool,

    #[arg(long = "no-context-files")]
    pub no_context_files: bool,

    #[arg(long, short = 'l')]
    pub local: bool,

    #[arg(long = "all")]
    pub all: bool,

    #[arg(long = "extensions")]
    pub update_extensions: bool,

    #[arg(long = "self")]
    pub update_self: bool,

    #[arg(long = "force")]
    pub force: bool,

    /// Upgrade bbarit itself to the latest published version, then exit.
    #[arg(long = "upgrade")]
    pub upgrade: bool,

    #[arg(long = "export")]
    pub export: Option<PathBuf>,

    #[arg(long, short = 'p')]
    pub print: bool,

    #[arg(long = "continue", short = 'c')]
    pub continue_session: bool,

    #[arg(long, short = 'r')]
    pub resume: bool,

    #[arg(long)]
    pub session: Option<String>,

    #[arg(long = "session-id")]
    pub session_id: Option<String>,

    #[arg(long)]
    pub fork: Option<String>,

    #[arg(long)]
    pub session_dir: Option<PathBuf>,

    #[arg(long)]
    pub no_session: bool,

    #[arg(long = "approve", short = 'a')]
    pub approve: bool,

    #[arg(long = "no-approve", short = 'A', alias = "na")]
    pub no_approve: bool,

    #[arg(long, short = 'n')]
    pub name: Option<String>,

    #[arg(long)]
    pub list_models: bool,

    #[arg(long)]
    pub list_providers: bool,

    #[arg(long)]
    pub tui: bool,

    #[arg(long)]
    pub no_tui: bool,

    /// Stream assistant output token-by-token (Anthropic). Default off.
    #[arg(long)]
    pub stream: bool,

    /// Skip the startup folder picker and use the current directory.
    #[arg(long = "no-pick")]
    pub no_pick: bool,

    /// Run each positional input as a parallel sub-agent task (multi-process).
    #[arg(long)]
    pub orchestrate: bool,

    #[arg(long, value_enum, default_value_t = OutputMode::Text)]
    pub mode: OutputMode,

    #[arg(value_name = "INPUT")]
    pub inputs: Vec<String>,
}

impl Cli {
    pub fn parse_args() -> Self {
        <Self as Parser>::parse_from(std::env::args_os().map(|arg| match arg.to_str() {
            Some("-na") => OsString::from("--no-approve"),
            Some("-nt") => OsString::from("--no-tools"),
            Some("-nbt") => OsString::from("--no-builtin-tools"),
            Some("-xt") => OsString::from("--exclude-tools"),
            Some("-ne") => OsString::from("--no-extensions"),
            Some("-ns") => OsString::from("--no-skills"),
            Some("-np") => OsString::from("--no-prompt-templates"),
            Some("-nc") => OsString::from("--no-context-files"),
            _ => arg,
        }))
    }

    pub fn initial_message(&self) -> Result<Option<String>> {
        let mut parts = Vec::new();
        // Only consume stdin when no inline message/command argument was given.
        // Otherwise a non-terminal stdin (e.g. a pipe that never closes) would
        // block, and piped content would be prepended ahead of an explicit
        // command, breaking slash-command routing.
        if self.inputs.is_empty() && !self.tui && !io::stdin().is_terminal() {
            let mut stdin = String::new();
            io::stdin()
                .read_to_string(&mut stdin)
                .context("failed to read stdin")?;
            if !stdin.trim().is_empty() {
                parts.push(stdin.trim().to_string());
            }
        }
        for input in &self.inputs {
            if let Some(rest) = input.strip_prefix('@') {
                // Only the first whitespace token is the @file reference — a
                // single quoted argument like "@shot.png what is this?" used to
                // be read as one giant filename and fail the whole run.
                let (path, trailing) = match rest.split_once(char::is_whitespace) {
                    Some((path, trailing)) => (path, trailing.trim()),
                    None => (rest, ""),
                };
                // Images are attached as vision input downstream, not inlined as
                // text — keep the token so the agent can load the file.
                let lower = path.to_lowercase();
                if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
                    .iter()
                    .any(|ext| lower.ends_with(ext))
                {
                    parts.push(format!("@{path}"));
                } else {
                    let text = std::fs::read_to_string(path)
                        .with_context(|| format!("failed to read @{path}"))?;
                    parts.push(format!("--- file: {path} ---\n{text}"));
                }
                if !trailing.is_empty() {
                    parts.push(trailing.to_string());
                }
            }
        }
        let messages = self
            .inputs
            .iter()
            .filter(|input| !input.starts_with('@'))
            .cloned()
            .collect::<Vec<_>>();
        if !messages.is_empty() {
            parts.push(messages.join(" "));
        }
        Ok((!parts.is_empty()).then(|| parts.join("\n\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single quoted argument "@shot.png what is this?" must attach the image
    /// token and keep the question — it used to be read as one giant filename
    /// and fail the whole headless run.
    #[test]
    fn at_image_argument_with_trailing_prompt_splits_cleanly() {
        let cli = Cli::parse_from(["bbarit-oss", "-p", "@shot.png what solid color is this?"]);
        let message = cli.initial_message().unwrap().unwrap();
        assert!(message.contains("@shot.png"));
        assert!(message.contains("what solid color is this?"));
        assert!(!message.contains("@shot.png what"));
    }
}
