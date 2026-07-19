//! Multi-process orchestrator: run several tasks concurrently, each in its own
//! `bbarit-oss --print` child process (true OS-level parallelism, isolated state),
//! then collect and print the results in order.
//!
//! Invoked with `--orchestrate` and one task string per positional input:
//!   bbarit-oss --orchestrate "add tests for foo" "document the api" "fix lint"

use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::cli::Cli;
use crate::config::AppConfig;

const MAX_PARALLEL: usize = 4;

/// Frame a sub-agent prompt so the child's final output ends with a compact,
/// machine-skimmable result block — the parent agent reads the tail of a long
/// transcript, so the essentials must be there, not scattered.
pub fn frame_subagent_prompt(prompt: &str) -> String {
    format!(
        "{prompt}\n\nWhen you are finished, END your reply with exactly this section:\n\
         == RESULT ==\n\
         - Outcome: [done | partial | blocked] + one-line summary\n\
         - Files changed: [comma-separated paths, or 'none']\n\
         - Verified: [what you ran and what it showed, or 'not verified']\n\
         - Notes for parent: [anything the caller must know, or 'none']"
    )
}

/// Spawn ONE sub-agent and run it to completion: a fresh `--print` child of THIS
/// executable, running `prompt` in `cwd`, returning its final output.
///
/// Two env vars matter for the internalized build (where the binary is the host
/// app, not a standalone binary):
/// - `BBARIT_AGENT_MODE=1` makes the host app re-exec as the agent, not the GUI.
/// - `BBARIT_SUBAGENT=1` marks the child as a sub-agent so it does NOT offer the
///   `task` tool itself — that caps nesting at one level (no sub-agent fork bombs).
pub fn run_subagent(
    cwd: &Path,
    prompt: &str,
    provider: Option<&str>,
    model: Option<&str>,
    approve: bool,
    persona: Option<&str>,
) -> String {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => return format!("subagent: cannot resolve executable: {error}"),
    };
    let mut command = crate::spawn::no_window_command(&exe);
    command
        .env("BBARIT_AGENT_MODE", "1")
        .env("BBARIT_SUBAGENT", "1")
        .arg("--print")
        .arg("--no-pick")
        .arg("--no-session");
    if let Some(persona) = persona {
        command.env("BBARIT_PERSONA", persona);
    }
    if let Some(provider) = provider {
        command.arg("--provider").arg(provider);
    }
    if let Some(model) = model {
        command.arg("--model").arg(model);
    }
    if approve {
        command.arg("--approve");
    }
    command
        .arg(frame_subagent_prompt(prompt))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Spawn + poll instead of `output()`: the child is a separate PROCESS, so
    // the parent's Esc/abort flag never reaches it — a blocking `output()` made
    // every team/orchestrate run un-cancellable until all sub-agents finished.
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return format!("(failed to launch sub-agent: {error})"),
    };
    // Drain pipes on helper threads so a chatty child can't deadlock on a full
    // pipe buffer while the poll loop below waits (same shape as run_shell).
    fn drain<R: Read + Send + 'static>(
        pipe: Option<R>,
        buf: Arc<Mutex<Vec<u8>>>,
        done: Arc<AtomicBool>,
    ) {
        thread::spawn(move || {
            if let Some(mut pipe) = pipe {
                let mut chunk = [0u8; 8192];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut guard) = buf.lock() {
                                guard.extend_from_slice(&chunk[..n]);
                            }
                        }
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
        });
    }
    let out_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let err_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let out_done = Arc::new(AtomicBool::new(false));
    let err_done = Arc::new(AtomicBool::new(false));
    drain(
        child.stdout.take(),
        Arc::clone(&out_buf),
        Arc::clone(&out_done),
    );
    drain(
        child.stderr.take(),
        Arc::clone(&err_buf),
        Arc::clone(&err_done),
    );

    let mut cancelled = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if crate::commands::cancel_requested() {
            crate::tools::kill_process_tree(&mut child);
            let _ = child.wait();
            cancelled = true;
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };

    // Normal exit hits pipe EOF within milliseconds; the deadline only caps a
    // killed child whose grandchildren still hold the pipe write ends open.
    let collect_deadline = Instant::now() + Duration::from_secs(2);
    while (!out_done.load(Ordering::Relaxed) || !err_done.load(Ordering::Relaxed))
        && Instant::now() < collect_deadline
    {
        thread::sleep(Duration::from_millis(20));
    }
    let mut combined = out_buf
        .lock()
        .map(|guard| String::from_utf8_lossy(&guard).into_owned())
        .unwrap_or_default();
    if cancelled {
        combined.push_str("\n[cancelled] sub-agent stopped by user request");
    } else if !status.is_some_and(|status| status.success()) {
        let err = err_buf
            .lock()
            .map(|guard| String::from_utf8_lossy(&guard).into_owned())
            .unwrap_or_default();
        if !err.trim().is_empty() {
            combined.push_str("\n[stderr] ");
            combined.push_str(err.trim());
        }
    }
    combined.trim().to_string()
}

/// True when THIS process is itself a sub-agent (spawned by `run_subagent`), so
/// callers can suppress further sub-agent spawning to cap nesting depth.
pub fn is_subagent() -> bool {
    std::env::var_os("BBARIT_SUBAGENT").is_some()
}

/// Run a TEAM of sub-agents in parallel (≤MAX_PARALLEL at a time), each on its
/// own prompt in `cwd`, and return their results joined in order. This is what
/// the `agent_team` tool calls so the model can fan a job out to several agents
/// at once. Sub-agents load their own default model/config.
pub fn run_team(
    cwd: &Path,
    tasks: &[String],
    provider: Option<&str>,
    model: Option<&str>,
    approve: bool,
    persona: Option<&str>,
) -> String {
    if tasks.is_empty() {
        return "agent_team: no tasks given".to_string();
    }
    let parallel = MAX_PARALLEL.min(tasks.len()).max(1);
    let mut results: Vec<(usize, String)> = Vec::new();
    let mut start = 0;
    while start < tasks.len() {
        // Esc: don't launch the next batch — in-flight sub-agents are killed by
        // run_subagent's own cancel poll, so joining them returns promptly.
        if crate::commands::cancel_requested() {
            break;
        }
        let end = (start + parallel).min(tasks.len());
        let handles: Vec<_> = (start..end)
            .map(|index| {
                let cwd = cwd.to_path_buf();
                let task = tasks[index].clone();
                let provider = provider.map(str::to_string);
                let model = model.map(str::to_string);
                let persona = persona.map(str::to_string);
                // Propagate the turn epoch so a hard abort (double-Esc) also
                // reads as cancelled inside subagent threads.
                let epoch = crate::commands::current_worker_epoch();
                thread::spawn(move || {
                    if let Some(epoch) = epoch {
                        crate::commands::enter_worker_epoch(epoch);
                    }
                    (
                        index,
                        run_subagent(
                            &cwd,
                            &task,
                            provider.as_deref(),
                            model.as_deref(),
                            approve,
                            persona.as_deref(),
                        ),
                    )
                })
            })
            .collect();
        for handle in handles {
            if let Ok(result) = handle.join() {
                results.push(result);
            }
        }
        start = end;
    }
    let skipped = tasks.len() - start.min(tasks.len());
    results.sort_by_key(|(index, _)| *index);
    let mut out = format!(
        "Agent team finished {} task(s) (up to {} in parallel):\n\n",
        results.len(),
        parallel
    );
    if skipped > 0 {
        out.push_str(&format!("(cancelled — {skipped} task(s) not started)\n\n"));
    }
    for (index, output) in &results {
        out.push_str(&format!(
            "=== Agent {} : {} ===\n",
            index + 1,
            tasks[*index]
        ));
        out.push_str(output.trim());
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

pub fn run(cli: &Cli, config: &AppConfig) -> Result<()> {
    let tasks: Vec<String> = cli.inputs.clone();
    if tasks.is_empty() {
        bail!("--orchestrate needs one or more task strings (each becomes a parallel sub-agent)");
    }
    print!("{}", run_tasks(config, cli.approve, &tasks));
    Ok(())
}

/// Run each task as its own `bbarit --print` child process (≤MAX_PARALLEL
/// concurrent) and return the results joined in order. Usable from the CLI
/// (`--orchestrate`) and the `/orchestrate` command.
pub fn run_tasks(config: &AppConfig, approve: bool, tasks: &[String]) -> String {
    let parallel = MAX_PARALLEL.min(tasks.len()).max(1);
    let mut results: Vec<(usize, String)> = Vec::new();
    let mut start = 0;
    while start < tasks.len() {
        if crate::commands::cancel_requested() {
            break;
        }
        let end = (start + parallel).min(tasks.len());
        let handles: Vec<_> = (start..end)
            .map(|index| {
                let provider = config.provider.clone();
                let model = config.model.clone();
                let cwd = config.cwd.clone();
                let task = tasks[index].clone();
                let epoch = crate::commands::current_worker_epoch();
                thread::spawn(move || {
                    if let Some(epoch) = epoch {
                        crate::commands::enter_worker_epoch(epoch);
                    }
                    let text = run_subagent(
                        &cwd,
                        &task,
                        Some(&provider),
                        model.as_deref(),
                        approve,
                        None,
                    );
                    (index, text)
                })
            })
            .collect();
        for handle in handles {
            if let Ok(result) = handle.join() {
                results.push(result);
            }
        }
        start = end;
    }
    let skipped = tasks.len() - start.min(tasks.len());

    results.sort_by_key(|(index, _)| *index);
    let mut out = format!(
        "Orchestrated {} task(s), up to {} in parallel:\n\n",
        results.len(),
        parallel
    );
    if skipped > 0 {
        out.push_str(&format!("(cancelled — {skipped} task(s) not started)\n\n"));
    }
    for (index, output) in &results {
        out.push_str(&format!("=== Task {} : {} ===\n", index + 1, tasks[*index]));
        out.push_str(output.trim());
        out.push_str("\n\n");
    }
    out
}
