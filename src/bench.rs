//! Self-benchmark: a built-in, machine-graded coding-task suite. `/bench`
//! materializes each task in a fresh temp directory, solves it with a
//! sub-agent (fresh context, same binary), grades it by running the task's
//! test file with node, and prints a scorecard. Because grading is the test's
//! exit code — not model self-report — the score is honest and repeatable,
//! which makes this the regression harness for agent-quality changes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

struct BenchTask {
    id: &'static str,
    title: &'static str,
    prompt: &'static str,
    files: &'static [(&'static str, &'static str)],
}

const TASKS: &[BenchTask] = &[
    BenchTask {
        id: "bugfix",
        title: "off-by-one bugfix",
        prompt: "test.js fails. Run `node test.js` to see why, fix calc.js IN PLACE so the tests pass. Do not create new files. Do not modify test.js.",
        files: &[
            (
                "calc.js",
                "// Range utilities used by the billing report.\nfunction sumRange(from, to) {\n  let total = 1;\n  for (let i = from; i < to; i++) {\n    total += i;\n  }\n  return total;\n}\n\nfunction averageRange(from, to) {\n  const count = to - from + 1;\n  return sumRange(from, to) / count;\n}\n\nmodule.exports = { sumRange, averageRange };\n",
            ),
            (
                "test.js",
                "const assert = require('assert');\nconst { sumRange, averageRange } = require('./calc');\nassert.strictEqual(sumRange(1, 3), 6);\nassert.strictEqual(sumRange(5, 5), 5);\nassert.strictEqual(sumRange(0, 0), 0);\nassert.strictEqual(averageRange(1, 3), 2);\nconsole.log('OK');\n",
            ),
        ],
    },
    BenchTask {
        id: "feature",
        title: "implement from spec",
        prompt: "Implement parseDuration in duration.js so `node test.js` passes. test.js IS the spec. Do not modify test.js.",
        files: &[
            (
                "duration.js",
                "// TODO: implement parseDuration. See test.js for the exact spec.\nfunction parseDuration(text) {\n  throw new Error('not implemented');\n}\nmodule.exports = { parseDuration };\n",
            ),
            (
                "test.js",
                "const assert = require('assert');\nconst { parseDuration } = require('./duration');\nassert.strictEqual(parseDuration('1h30m'), 5400);\nassert.strictEqual(parseDuration('45s'), 45);\nassert.strictEqual(parseDuration('2h'), 7200);\nassert.strictEqual(parseDuration('1h2m3s'), 3723);\nassert.strictEqual(parseDuration(''), 0);\nassert.throws(() => parseDuration('abc'), /invalid/i);\nassert.throws(() => parseDuration('3x'), /invalid/i);\nconsole.log('OK');\n",
            ),
        ],
    },
    BenchTask {
        id: "multifile",
        title: "cross-file requirement",
        prompt: "New requirement: a 'refund' event kind exists with severity 3. Modify the files under this directory IN PLACE so `node test.js` passes. Do not modify test.js.",
        files: &[
            (
                "lib/events.js",
                "const SEVERITY = { login: 1, logout: 1, purchase: 2, error: 3 };\n\nfunction severityOf(kind) {\n  if (!(kind in SEVERITY)) throw new Error(`unknown event kind: ${kind}`);\n  return SEVERITY[kind];\n}\n\nmodule.exports = { severityOf, SEVERITY };\n",
            ),
            (
                "report.js",
                "const { severityOf } = require('./lib/events');\n\nfunction importantEvents(events, minSeverity) {\n  return events\n    .filter((e) => severityOf(e.kind) >= minSeverity)\n    .sort((a, b) => b.at - a.at);\n}\n\nmodule.exports = { importantEvents };\n",
            ),
            (
                "test.js",
                "const assert = require('assert');\nconst { severityOf } = require('./lib/events');\nconst { importantEvents } = require('./report');\nassert.strictEqual(severityOf('refund'), 3);\nassert.strictEqual(severityOf('purchase'), 2);\nconst events = [\n  { kind: 'login', at: 1 },\n  { kind: 'refund', at: 5 },\n  { kind: 'error', at: 3 },\n  { kind: 'purchase', at: 2 },\n];\nassert.deepStrictEqual(importantEvents(events, 3).map((e) => e.kind), ['refund', 'error']);\nconsole.log('OK');\n",
            ),
        ],
    },
    BenchTask {
        id: "async",
        title: "async sequencing bug",
        prompt: "test.js fails. Fix the bug in queue.js IN PLACE so `node test.js` passes (execution ORDER is verified). Do not modify test.js.",
        files: &[
            (
                "queue.js",
                "// Job queue: run tasks sequentially, collect results, skip failures.\nasync function runQueue(tasks) {\n  const results = [];\n  for (const task of tasks) {\n    task().then((r) => results.push(r)).catch(() => results.push(null));\n  }\n  return results;\n}\nmodule.exports = { runQueue };\n",
            ),
            (
                "test.js",
                "const assert = require('assert');\nconst { runQueue } = require('./queue');\n(async () => {\n  const order = [];\n  const mk = (n, ms, fail) => () => new Promise((res, rej) => setTimeout(() => { order.push(n); (fail ? rej : res)(n); }, ms));\n  const results = await runQueue([mk(1, 30), mk(2, 10), mk(3, 1, true), mk(4, 5)]);\n  assert.deepStrictEqual(results, [1, 2, null, 4]);\n  assert.deepStrictEqual(order, [1, 2, 3, 4]);\n  console.log('OK');\n})().catch((e) => { console.error(e.message); process.exit(1); });\n",
            ),
        ],
    },
    BenchTask {
        id: "validation",
        title: "cross-file validation",
        prompt: "A new requirement is encoded in test.js. Modify the files under src/ IN PLACE so `node test.js` passes. Do not modify test.js.",
        files: &[
            (
                "src/config.js",
                "const DEFAULTS = { retries: 3, timeoutMs: 1000 };\nfunction getConfig(overrides) {\n  return Object.assign({}, DEFAULTS, overrides || {});\n}\nmodule.exports = { getConfig, DEFAULTS };\n",
            ),
            (
                "src/client.js",
                "const { getConfig } = require('./config');\nfunction buildClient(overrides) {\n  const cfg = getConfig(overrides);\n  return { attempts: cfg.retries + 1, deadline: cfg.timeoutMs * (cfg.retries + 1) };\n}\nmodule.exports = { buildClient };\n",
            ),
            (
                "test.js",
                "const assert = require('assert');\nconst { getConfig } = require('./src/config');\nconst { buildClient } = require('./src/client');\nassert.throws(() => getConfig({ retries: -1 }), RangeError);\nassert.throws(() => getConfig({ timeoutMs: 0 }), RangeError);\nassert.deepStrictEqual(getConfig({ retries: 5 }), { retries: 5, timeoutMs: 1000 });\nconst client = buildClient({ retries: 2, timeoutMs: 500 });\nassert.strictEqual(client.attempts, 3);\nassert.strictEqual(client.deadline, 1500);\nconsole.log('OK');\n",
            ),
        ],
    },
    BenchTask {
        id: "parser",
        title: "stateful CSV parser",
        prompt: "Implement parseCsvLine in csv.js so `node test.js` passes. test.js IS the spec (quoted fields, escaped quotes, edge cases). Do not modify test.js.",
        files: &[
            (
                "csv.js",
                "// TODO: implement parseCsvLine per test.js.\nfunction parseCsvLine(line) {\n  throw new Error('not implemented');\n}\nmodule.exports = { parseCsvLine };\n",
            ),
            (
                "test.js",
                "const assert = require('assert');\nconst { parseCsvLine } = require('./csv');\nassert.deepStrictEqual(parseCsvLine('a,b,c'), ['a', 'b', 'c']);\nassert.deepStrictEqual(parseCsvLine('\"a,b\",c'), ['a,b', 'c']);\nassert.deepStrictEqual(parseCsvLine('a,\"he said \"\"hi\"\"\",c'), ['a', 'he said \"hi\"', 'c']);\nassert.deepStrictEqual(parseCsvLine(''), ['']);\nassert.deepStrictEqual(parseCsvLine('a,,c'), ['a', '', 'c']);\nassert.throws(() => parseCsvLine('\"unterminated'), /unterminated/i);\nconsole.log('OK');\n",
            ),
        ],
    },
];

fn materialize(task: &BenchTask, root: &Path) -> Result<PathBuf> {
    let dir = root.join(task.id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for (name, content) in task.files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
    }
    Ok(dir)
}

fn grade(dir: &Path) -> bool {
    crate::spawn::no_window_command("node")
        .arg("test.js")
        .current_dir(dir)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Run the whole suite (or the tasks whose ids contain `filter`) and return a
/// scorecard. Each task is solved by a fresh sub-agent in its own directory.
pub fn run(config: &crate::config::AppConfig, filter: &str) -> Result<String> {
    let root = std::env::temp_dir().join("bbarit-selfbench");
    std::fs::create_dir_all(&root).context("create bench root")?;
    let filter = filter.trim();
    let selected: Vec<&BenchTask> = TASKS
        .iter()
        .filter(|t| filter.is_empty() || t.id.contains(filter))
        .collect();
    if selected.is_empty() {
        anyhow::bail!(
            "no bench task matches {filter:?} — available: {}",
            TASKS.iter().map(|t| t.id).collect::<Vec<_>>().join(", ")
        );
    }

    let mut lines = vec![format!(
        "Self-benchmark: {} task(s), model {}/{} — grading is `node test.js` exit code.",
        selected.len(),
        config.provider,
        config.model.as_deref().unwrap_or("(default)")
    )];
    let mut passed = 0usize;
    for task in &selected {
        crate::llm::emit_activity(&format!("\n⚙ bench {} — {}\n", task.id, task.title));
        let dir = materialize(task, &root)?;
        let _ = crate::orchestrator::run_subagent(
            &dir,
            task.prompt,
            Some(&config.provider),
            config.model.as_deref(),
            true,
            None,
        );
        let ok = grade(&dir);
        if ok {
            passed += 1;
        }
        crate::llm::emit_activity(&format!(
            "{} bench {}: {}\n",
            if ok { "✓" } else { "✗" },
            task.id,
            if ok { "PASS" } else { "FAIL" }
        ));
        lines.push(format!(
            "  {} {:<11} {}",
            if ok { "✓" } else { "✗" },
            task.id,
            task.title
        ));
    }
    lines.push(format!(
        "SCORE: {passed}/{} — workspaces kept under {} for inspection.",
        selected.len(),
        root.display()
    ));
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_materialize_and_initially_fail() {
        let root = std::env::temp_dir().join("bbarit-selfbench-test");
        for task in TASKS {
            let dir = materialize(task, &root).unwrap();
            // Every task must start RED — a task that passes untouched grades nothing.
            assert!(
                !grade(&dir),
                "task {} must fail before the agent works",
                task.id
            );
        }
    }

    #[test]
    fn task_ids_are_unique() {
        let mut ids: Vec<&str> = TASKS.iter().map(|t| t.id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }
}
