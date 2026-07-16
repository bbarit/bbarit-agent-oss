#![recursion_limit = "256"]

//! Library entrypoint for the bbarit-oss coding agent. The thin `src/main.rs`
//! binary forwards to [`run`]; keeping the logic in a library also makes the
//! whole agent embeddable as a crate.
pub mod auth;
pub mod bench;
pub mod checkpoints;
pub mod cli;
pub mod commands;
pub mod computer;
pub mod config;
pub mod extensions;
pub mod hashline;
pub mod hooks;
pub mod keybindings;
pub mod llm;
pub mod lsp;
pub mod mcp;
pub mod memory;
pub mod orchestrator;
pub mod package_cli;
pub mod personas;
pub mod project;
pub mod providers;
pub mod resources;
pub mod session;
pub mod spawn;
pub mod stream_ui;
pub mod syntax;
pub mod themes;
pub mod tools;
pub mod trust;
pub mod tui;
pub mod update;
pub mod usage;
pub mod websearch;
pub mod wiki;

use crate::cli::{Cli, OutputMode};
use crate::commands::{handle_input, print_help};
use crate::config::AppConfig;
use crate::providers::Registry;
use crate::session::{CURRENT_SESSION_VERSION, Message, Role, SessionStore};
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Run the BBARIT agent CLI/TUI. Parses args from the process environment and
/// blocks until the agent exits.
pub fn run() -> Result<()> {
    let cli = Cli::parse_args();
    if cli.upgrade {
        return update::run();
    }
    project::maybe_pick(&cli)?;
    let mut config = AppConfig::load(&cli)?;
    if let Some(output) = package_cli::handle(&cli, &config)? {
        println!("{output}");
        return Ok(());
    }
    if cli.orchestrate {
        return orchestrator::run(&cli, &config);
    }
    // Persona injection at startup: --persona flag, BBARIT_PERSONA env (how a
    // launcher or parent agent assigns one), or the settings default — so a
    // terminal can open already fully in character.
    let startup_persona = cli
        .persona
        .clone()
        .or_else(|| std::env::var("BBARIT_PERSONA").ok())
        .or_else(|| config.default_persona.clone());
    if let Some(requested) = startup_persona {
        match personas::find_persona(&config, &requested) {
            Ok(persona) => {
                eprintln!(
                    "persona: {} {} ({})",
                    persona.emoji, persona.name, persona.id
                );
                personas::adopt(persona);
            }
            Err(error) => eprintln!("persona: {error}"),
        }
    }
    let registry = Registry::load(&config)?;

    if cli.list_providers {
        for provider in registry.providers() {
            println!(
                "{}\t{}\t{} models",
                provider.id,
                provider.name,
                registry.models_for_provider(&provider.id).len()
            );
        }
        return Ok(());
    }

    if cli.list_models {
        let query = cli
            .model
            .as_deref()
            .or_else(|| cli.inputs.first().map(String::as_str))
            .unwrap_or("");
        for model in registry.search_models(query) {
            println!(
                "{}\t{}\t{}\t{}",
                model.provider, model.id, model.api, model.name
            );
        }
        return Ok(());
    }

    if let Some(source) = &cli.export {
        let target = export_target(source, cli.inputs.first().map(String::as_str));
        SessionStore::export_session_ref_html(&config, &source.display().to_string(), &target)?;
        println!("Exported HTML {}", target.display());
        return Ok(());
    }

    let mut store = SessionStore::open(&config, &cli)?;
    if let Some(name) = &cli.name {
        store.set_name(name)?;
    }
    // Long-lived modes: start building the code-context index now so the first
    // turn's auto-RAG already has it (the build itself never blocks a turn).
    if config.project_trusted && commands::auto_code_context_enabled() {
        tools::warm_code_index(&config.cwd);
    }
    let initial = cli.initial_message()?;
    if cli.mode == OutputMode::Json {
        return run_json_mode(&mut store, &registry, &config, initial);
    }
    if cli.print
        || (!cli.tui && (!atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stdout)))
    {
        if let Some(input) = initial {
            handle_and_print(&mut store, &registry, &config, &input)?;
        }
        return Ok(());
    }

    if !cli.no_tui {
        return crate::tui::run_interactive(&mut store, &registry, &mut config, initial);
    }

    // Fallback line REPL (--no-tui): also a fresh interactive session, so drop any
    // goal carried over from a previous one before the first prompt.
    commands::reset_goal_for_new_session(&config);
    println!("bbarit-oss {}", env!("CARGO_PKG_VERSION"));
    println!("bbarit-oss agent. /help, /providers, /models, /model, /exit");
    println!("Session: {}", store.session().id);
    println!();

    if let Some(input) = initial {
        handle_and_print(&mut store, &registry, &config, &input)?;
    }

    loop {
        print!("bbarit-oss> ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" {
            break;
        }
        if input == "/help" {
            println!("{}", print_help());
            continue;
        }
        if let Err(error) = handle_and_print(&mut store, &registry, &config, input) {
            eprintln!("Error: {error:#}");
        }
    }

    Ok(())
}

/// Run one prompt and print the result. When streaming is enabled the assistant
/// text is printed live by the stream sink; otherwise the bundled output is
/// printed once at the end.
fn handle_and_print(
    store: &mut SessionStore,
    registry: &Registry,
    config: &AppConfig,
    input: &str,
) -> Result<()> {
    use std::io::Write;

    if !config.stream {
        let output = handle_input(store, registry, config, input)?;
        println!("{output}");
        return Ok(());
    }

    // On a real terminal, show a spinner while waiting then stream tokens. When
    // piped, stream raw text only (no spinner/control characters).
    if atty::is(atty::Stream::Stdout) {
        let guard = crate::stream_ui::start(String::new());
        let result = handle_input(store, registry, config, input);
        let streamed = guard.finish();
        let output = result?;
        if streamed {
            println!();
        } else {
            println!("{output}");
        }
        return Ok(());
    }

    // Piped (a program is consuming us): keep stdout machine-readable — the
    // final assistant text only. Live tokens and activity lines still stream,
    // but to stderr, so `bbarit-oss --print ... | consumer` never sees narration,
    // thinking summaries, or "⚙ tool" lines mixed into the answer.
    crate::llm::set_stream_sink(Some(Box::new(move |chunk: &str| {
        eprint!("{chunk}");
        let _ = std::io::stderr().flush();
    })));
    let result = handle_input(store, registry, config, input);
    crate::llm::set_stream_sink(None);
    let output = result?;
    eprintln!();
    println!("{output}");
    Ok(())
}

fn run_json_mode(
    store: &mut SessionStore,
    registry: &Registry,
    config: &AppConfig,
    initial: Option<String>,
) -> Result<()> {
    print_json_line(json!({
        "type": "session",
        "version": CURRENT_SESSION_VERSION,
        "id": store.session().id,
        "timestamp": store.session().created_at,
        "cwd": store.session().cwd.display().to_string(),
    }))?;

    let Some(input) = initial else {
        return Ok(());
    };

    print_json_line(json!({ "type": "agent_start" }))?;
    print_json_line(json!({ "type": "turn_start" }))?;
    let before = store.messages().len();
    if config.stream {
        crate::llm::set_stream_sink(Some(Box::new(|chunk: &str| {
            let line = json!({
                "type": "message_update",
                "delta": { "type": "text_delta", "text": chunk },
            });
            println!("{line}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        })));
    }
    let result = handle_input(store, registry, config, &input);
    crate::llm::set_stream_sink(None);
    match result {
        Ok(output) => {
            let new_messages = store.messages()[before..].to_vec();
            if new_messages.is_empty() {
                let synthetic = json_message("assistant", &output);
                print_json_line(json!({ "type": "message_start", "message": synthetic }))?;
                print_json_line(json!({ "type": "message_end", "message": synthetic }))?;
                print_json_line(json!({
                    "type": "turn_end",
                    "message": synthetic,
                    "toolResults": [],
                }))?;
            } else {
                for message in &new_messages {
                    print_json_line(json!({ "type": "message_end", "message": message }))?;
                }
                if let Some(last) = new_messages.last() {
                    print_json_line(json!({
                        "type": "turn_end",
                        "message": last,
                        "toolResults": new_messages
                            .iter()
                            .filter(|message| message.role == Role::Tool)
                            .collect::<Vec<&Message>>(),
                    }))?;
                }
            }
            print_json_line(json!({
                "type": "agent_end",
                "messages": store.messages(),
            }))?;
            Ok(())
        }
        Err(error) => {
            let message = json!({
                "role": "assistant",
                "content": format!("{error:#}"),
                "isError": true,
                "created_at": Utc::now().to_rfc3339(),
            });
            print_json_line(json!({
                "type": "turn_end",
                "message": message,
                "toolResults": [],
                "errorMessage": format!("{error:#}"),
            }))?;
            print_json_line(json!({
                "type": "agent_end",
                "messages": store.messages(),
                "errorMessage": format!("{error:#}"),
            }))?;
            Err(error)
        }
    }
}

fn print_json_line(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

fn json_message(role: &str, content: &str) -> serde_json::Value {
    json!({
        "role": role,
        "content": content,
        "created_at": Utc::now().to_rfc3339(),
    })
}

fn export_target(source: &Path, requested: Option<&str>) -> PathBuf {
    if let Some(requested) = requested.filter(|value| !value.trim().is_empty()) {
        return PathBuf::from(requested);
    }
    let mut target = source.to_path_buf();
    target.set_extension("html");
    target
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
