//! Compress build / test / install / CI output for AI agents.
//!
//! `semble_rs digest` strips progress noise, groups warnings, and collapses
//! successful CI steps so an agent sees the actual failure (often 1-3% of
//! the raw bytes) instead of megabytes of compile / install chatter.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Cargo,
    Pnpm,
    Tsc,
    Pytest,
    Ci,
    GoTest,
    Gradle,
    Ruff,
    Mypy,
    Compiler, // clang / gcc / make / cmake / swiftc — share the file:line:col error format
    Unknown,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cargo" | "rust" => Some(Format::Cargo),
            "pnpm" | "npm" | "yarn" | "node" | "bun" => Some(Format::Pnpm),
            "tsc" | "typescript" => Some(Format::Tsc),
            "pytest" | "python" => Some(Format::Pytest),
            "ci" | "gha" | "github-actions" | "actions" => Some(Format::Ci),
            "go" | "gotest" | "go-test" => Some(Format::GoTest),
            "gradle" => Some(Format::Gradle),
            "ruff" => Some(Format::Ruff),
            "mypy" => Some(Format::Mypy),
            "compiler" | "clang" | "gcc" | "cmake" | "make" | "swift" | "swiftc" => {
                Some(Format::Compiler)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Format::Cargo => "cargo",
            Format::Pnpm => "pnpm",
            Format::Tsc => "tsc",
            Format::Pytest => "pytest",
            Format::Ci => "ci",
            Format::GoTest => "go",
            Format::Gradle => "gradle",
            Format::Ruff => "ruff",
            Format::Mypy => "mypy",
            Format::Compiler => "compiler",
            Format::Unknown => "unknown",
        }
    }
}

// ---------- shared ----------

static ANSI_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());

fn strip_ansi(text: &str) -> String {
    ANSI_RE.replace_all(text, "").into_owned()
}

// ---------- detection ----------

static GHA_PREFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[^\t\n]+\t[^\t\n]+\t\u{feff}?\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z\s?")
        .unwrap()
});
static CARGO_COMPILE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^\s*Compiling \S+ v[\d.]+").unwrap());
static CARGO_TEST_RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^running \d+ tests?$").unwrap());
static TSC_PAREN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^.+\.[jt]sx?\(\d+,\d+\): error TS\d+").unwrap());
static TSC_COLON: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^.+\.[jt]sx?:\d+:\d+ - error TS\d+").unwrap());
static PNPM_RESOLVED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^Progress: resolved \d+").unwrap());
static GO_TEST_RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^=== RUN\s+Test").unwrap());
static GO_TEST_RESULT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^---\s+(PASS|FAIL):").unwrap());
static GRADLE_TASK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^> Task :").unwrap());
static GRADLE_BUILD_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^BUILD (SUCCESSFUL|FAILED)").unwrap());
// Ruff: file:line:col: RULE_CODE ... (RULE = letter+digits like F401, E741)
static RUFF_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^[^:\s]+\.py:\d+:\d+:\s+[A-Z]+\d+\b").unwrap());
static MYPY_ERROR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^\S+\.py(?::\d+)?:\s+(error|note):").unwrap());
// clang/gcc/cmake/swift compiler error: <path>:<line>:<col>: error/warning:
static COMPILER_DIAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^/?[^:\n]+\.(?:c|cpp|cc|cxx|h|hpp|m|mm|swift|rs|go|java|kt):\d+:\d+:\s+(?:error|warning|note|fatal error):").unwrap()
});
static CMAKE_BUILDING: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^\[\s*\d+%\]\s+(Building|Linking|Generating)").unwrap());
static SWIFT_BUILD_STEP: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\[\d+/\d+\]\s+\S+").unwrap());

pub fn detect(text: &str) -> Format {
    let plain = strip_ansi(text);
    if GHA_PREFIX.is_match(&plain)
        || plain.contains("##[group]")
        || plain.contains("##[error]")
        || plain.contains("##[section]")
    {
        return Format::Ci;
    }
    if CARGO_COMPILE.is_match(&plain) {
        return Format::Cargo;
    }
    if CARGO_TEST_RUN.is_match(&plain) && plain.contains("test result:") {
        return Format::Cargo;
    }
    if GO_TEST_RUN.is_match(&plain) && GO_TEST_RESULT.is_match(&plain) {
        return Format::GoTest;
    }
    if GRADLE_TASK.is_match(&plain) || GRADLE_BUILD_LINE.is_match(&plain) {
        return Format::Gradle;
    }
    if TSC_PAREN.is_match(&plain) || TSC_COLON.is_match(&plain) {
        return Format::Tsc;
    }
    let looks_like_pytest = plain.contains("test session starts")
        || plain.contains("=== FAILURES ===")
        || plain.contains(".py::test")
        || plain.contains("pytest");
    if looks_like_pytest
        && (plain.contains("FAILED") || plain.contains("PASSED") || plain.contains("passed in"))
    {
        return Format::Pytest;
    }
    if RUFF_LINE.is_match(&plain) {
        return Format::Ruff;
    }
    if MYPY_ERROR.is_match(&plain) {
        return Format::Mypy;
    }
    if COMPILER_DIAG.is_match(&plain)
        || CMAKE_BUILDING.is_match(&plain)
        || SWIFT_BUILD_STEP.is_match(&plain)
    {
        return Format::Compiler;
    }
    if plain.contains("pnpm v")
        || PNPM_RESOLVED.is_match(&plain)
        || plain.contains("npm warn")
        || plain.contains("bun install")
    {
        return Format::Pnpm;
    }
    Format::Unknown
}

pub fn digest(text: &str, fmt: Format) -> String {
    match fmt {
        Format::Cargo => digest_cargo(text),
        Format::Pnpm => digest_pnpm(text),
        Format::Tsc => digest_tsc(text),
        Format::Pytest => digest_pytest(text),
        Format::Ci => digest_ci(text),
        Format::GoTest => digest_go_test(text),
        Format::Gradle => digest_gradle(text),
        Format::Ruff => digest_ruff(text),
        Format::Mypy => digest_mypy(text),
        Format::Compiler => digest_compiler(text),
        Format::Unknown => text.trim_end().to_string(),
    }
}

// ---------- cargo build / test ----------

static CARGO_PROGRESS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"^\s*Compiling\s",
        r"^\s*Downloaded\s",
        r"^\s*Downloading",
        r"^\s*Updating\s",
        r"^\s*Fetching",
        r"^\s*Adding\s",
        r"^\s*Documenting",
        r"^\s*Generated\s",
        r"^\s*Checking\s",
        r"^\s*Locking\s",
        r"^\s*Blocking",
        r"^\s*Finished\s",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});
static CARGO_TEST_PASS: Lazy<Regex> = Lazy::new(|| Regex::new(r"^test \S+ \.\.\. ok$").unwrap());
static CARGO_TEST_RUN_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^running \d+ tests?$").unwrap());
static CARGO_DOCTEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*Doc-tests\s").unwrap());
static CARGO_RUNNING_BIN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s+Running\s").unwrap());

pub fn digest_cargo(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut n_pass_tests = 0usize;
    let mut finished_line: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        let trimmed = line.trim();

        let is_progress = CARGO_PROGRESS.iter().any(|p| p.is_match(line));
        if is_progress {
            if line.contains("Finished") {
                finished_line = Some(trimmed.to_string());
            }
            continue;
        }
        if CARGO_TEST_PASS.is_match(trimmed) {
            n_pass_tests += 1;
            continue;
        }
        if CARGO_TEST_RUN_LINE.is_match(trimmed) {
            continue;
        }
        if CARGO_DOCTEST.is_match(line) {
            continue;
        }
        if CARGO_RUNNING_BIN.is_match(line) {
            continue;
        }
        kept.push(line.to_string());
    }

    let mut head: Vec<String> = Vec::new();
    if let Some(f) = finished_line {
        head.push(f);
    }
    if n_pass_tests > 0 {
        head.push(format!("({} passing tests stripped)", n_pass_tests));
    }
    head.extend(kept);
    head.join("\n").trim().to_string()
}

// ---------- pnpm / npm / yarn ----------

static PNPM_PROGRESS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"^Progress:",
        r"^Already up to date",
        r"^Lockfile is up to date",
        r"^Packages: ",
        r"^[\+\-]{2,}\s*$",
        r"^node_modules/\.pnpm",
        r"^Resolving:",
        r"^Fetched\s",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});
static PNPM_BANNER: Lazy<Vec<Regex>> = Lazy::new(|| {
    [r"^\s*╭", r"^\s*│", r"^\s*╰"]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
});
static PNPM_DEPRECATED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:npm\s+)?warn\s+deprecated\s+(\S+)").unwrap());

pub fn digest_pnpm(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut deprecation_counts: HashMap<String, usize> = HashMap::new();

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if PNPM_BANNER.iter().any(|p| p.is_match(line)) {
            continue;
        }
        if PNPM_PROGRESS.iter().any(|p| p.is_match(line)) {
            continue;
        }
        if let Some(caps) = PNPM_DEPRECATED.captures(line) {
            *deprecation_counts.entry(caps[1].to_string()).or_insert(0) += 1;
            continue;
        }
        kept.push(line.to_string());
    }

    if !deprecation_counts.is_empty() {
        kept.push(String::new());
        kept.push(format!(
            "-- deprecated packages: {} unique --",
            deprecation_counts.len()
        ));
        let mut items: Vec<(&String, &usize)> = deprecation_counts.iter().collect();
        items.sort_by(|a, b| a.0.cmp(b.0));
        for (pkg, n) in items.iter().take(5) {
            kept.push(if **n > 1 {
                format!("  {} x{}", pkg, n)
            } else {
                format!("  {}", pkg)
            });
        }
        if items.len() > 5 {
            kept.push(format!("  ... +{} more", items.len() - 5));
        }
    }
    kept.join("\n").trim().to_string()
}

// ---------- tsc ----------

static TSC_PAREN_PARSE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(.+\.[jt]sx?)\((\d+),(\d+)\): error (TS\d+): (.+)$").unwrap());
static TSC_COLON_PARSE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(.+\.[jt]sx?):(\d+):(\d+) - error (TS\d+): (.+)$").unwrap());

pub fn digest_tsc(text: &str) -> String {
    let text = strip_ansi(text);
    let mut by_code: HashMap<String, Vec<(String, usize, String)>> = HashMap::new();
    let mut other: Vec<String> = Vec::new();
    let mut total = 0usize;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let caps = TSC_PAREN_PARSE
            .captures(line)
            .or_else(|| TSC_COLON_PARSE.captures(line));
        if let Some(caps) = caps {
            total += 1;
            let file = caps.get(1).unwrap().as_str().to_string();
            let lineno: usize = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            let code = caps.get(4).unwrap().as_str().to_string();
            let msg = caps.get(5).unwrap().as_str().trim().to_string();
            by_code.entry(code).or_default().push((file, lineno, msg));
        } else {
            other.push(line.to_string());
        }
    }

    let mut out: Vec<String> = other;
    if total > 0 {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(format!(
            "=== TypeScript errors: {} total, {} unique codes ===",
            total,
            by_code.len()
        ));
        let mut codes: Vec<(&String, &Vec<(String, usize, String)>)> = by_code.iter().collect();
        codes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (code, items) in codes {
            out.push(format!("[{}] {} errors:", code, items.len()));
            for (file, ln, msg) in items.iter().take(3) {
                out.push(format!("  {}:{}: {}", file, ln, msg));
            }
            if items.len() > 3 {
                out.push(format!("  ... +{} more", items.len() - 3));
            }
        }
    }
    out.join("\n").trim().to_string()
}

// ---------- pytest ----------

static PYTEST_PROGRESS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"^cachedir:",
        r"^plugins:",
        r"^collecting",
        r"^platform ",
        r"^rootdir:",
        r"^Installed \d+ packages",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});
static PYTEST_PASS: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\S+\.py::\S+\s+PASSED").unwrap());

pub fn digest_pytest(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut n_pass = 0usize;

    for raw in text.lines() {
        let line = raw.trim_end();
        if PYTEST_PROGRESS.iter().any(|p| p.is_match(line)) {
            continue;
        }
        if PYTEST_PASS.is_match(line) {
            n_pass += 1;
            continue;
        }
        kept.push(line.to_string());
    }

    if n_pass > 0 {
        kept.insert(0, format!("({} passing tests stripped)", n_pass));
    }
    kept.join("\n").trim().to_string()
}

// ---------- GitHub Actions / CI ----------

static GHA_BARE_TS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\u{feff}?\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z\s?").unwrap());
static GHA_GROUP_START: Lazy<Regex> = Lazy::new(|| Regex::new(r"##\[group\](.*)$").unwrap());
static GHA_GROUP_END: Lazy<Regex> = Lazy::new(|| Regex::new(r"##\[endgroup\]").unwrap());
static GHA_ERROR_MARK: Lazy<Regex> = Lazy::new(|| Regex::new(r"##\[error\]").unwrap());
static GHA_WARNING_MARK: Lazy<Regex> = Lazy::new(|| Regex::new(r"##\[warning\]").unwrap());
static GHA_DEBUG_MARK: Lazy<Regex> = Lazy::new(|| Regex::new(r"##\[debug\]").unwrap());
static GHA_SECTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"^##\[section\]").unwrap());
static CI_ERROR_KEYWORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(error|FAIL|failed|panic|fatal|cannot find|undefined reference|exit code [1-9])\b",
    )
    .unwrap()
});

const CI_FAILED_GROUP_TAIL: usize = 80;

fn strip_gha_prefix(line: &str) -> &str {
    if let Some(m) = GHA_PREFIX.find(line) {
        &line[m.end()..]
    } else if let Some(m) = GHA_BARE_TS.find(line) {
        &line[m.end()..]
    } else {
        line
    }
}

pub fn digest_ci(text: &str) -> String {
    let text = strip_ansi(text);
    let mut out: Vec<String> = Vec::new();
    let mut in_group = false;
    let mut group_name = String::new();
    let mut group_lines: Vec<String> = Vec::new();
    let mut group_has_error = false;

    let flush_group =
        |out: &mut Vec<String>, name: &str, lines: &[String], has_error: bool, still_open: bool| {
            if has_error {
                let tail_n = std::cmp::min(CI_FAILED_GROUP_TAIL, lines.len());
                let start = lines.len() - tail_n;
                if still_open {
                    out.push(format!(
                        "━━━ FAILED GROUP: {} (still open, tail {}) ━━━",
                        name, tail_n
                    ));
                } else {
                    out.push(format!(
                        "━━━ FAILED GROUP: {} ({} lines, tail {}) ━━━",
                        name,
                        lines.len(),
                        tail_n
                    ));
                }
                out.extend(lines[start..].iter().cloned());
                if !still_open {
                    out.push(format!("━━━ END {} ━━━", name));
                }
            } else {
                out.push(format!("[ok] {} ({} lines)", name, lines.len()));
            }
        };

    for raw in text.lines() {
        let line = strip_gha_prefix(raw).trim_end();
        if line.is_empty() {
            continue;
        }
        if GHA_DEBUG_MARK.is_match(line) {
            continue;
        }
        if GHA_SECTION.is_match(line) {
            if in_group {
                group_lines.push(line.to_string());
            } else {
                out.push(line.to_string());
            }
            continue;
        }
        if let Some(caps) = GHA_GROUP_START.captures(line) {
            in_group = true;
            group_name = caps
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            if group_name.is_empty() {
                group_name = "(unnamed)".to_string();
            }
            group_lines.clear();
            group_has_error = false;
            continue;
        }
        if GHA_GROUP_END.is_match(line) {
            if in_group {
                flush_group(&mut out, &group_name, &group_lines, group_has_error, false);
                in_group = false;
                group_lines.clear();
            }
            continue;
        }
        if GHA_ERROR_MARK.is_match(line) {
            group_has_error = true;
            if in_group {
                group_lines.push(line.to_string());
            } else {
                out.push(line.to_string());
            }
            continue;
        }
        if GHA_WARNING_MARK.is_match(line) {
            if in_group {
                group_lines.push(line.to_string());
            } else {
                out.push(line.to_string());
            }
            continue;
        }
        if CI_ERROR_KEYWORDS.is_match(line) {
            group_has_error = true;
        }
        if in_group {
            group_lines.push(line.to_string());
        } else {
            out.push(line.to_string());
        }
    }

    if in_group && !group_lines.is_empty() {
        flush_group(&mut out, &group_name, &group_lines, group_has_error, true);
    }

    out.join("\n").trim().to_string()
}

// ---------- go test ----------

static GO_RUN_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^=== RUN\s+\S+").unwrap());
static GO_PASS_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^---\s+PASS:\s+\S+\s+\(\S+\)").unwrap());
static GO_SKIP_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^---\s+SKIP:\s+\S+\s+\(\S+\)").unwrap());
// Framework stack frames we collapse to keep the user's frames clear.
static GO_FRAMEWORK_FRAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:testing\.|runtime\.|created by testing\.)").unwrap());
static GO_FRAMEWORK_PATH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*/\S*/(?:src/(?:testing|runtime)/)\S+\.go:\d+").unwrap());

pub fn digest_go_test(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut n_pass = 0usize;
    let mut n_skip = 0usize;
    let mut framework_frames_skipped = 0usize;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        if GO_RUN_LINE.is_match(line) {
            continue;
        }
        if GO_PASS_LINE.is_match(line) {
            n_pass += 1;
            continue;
        }
        if GO_SKIP_LINE.is_match(line) {
            n_skip += 1;
            continue;
        }
        if GO_FRAMEWORK_FRAME.is_match(line) || GO_FRAMEWORK_PATH.is_match(line) {
            framework_frames_skipped += 1;
            continue;
        }
        // keep --- FAIL, panic, user code frames, file:line: errors, summary
        kept.push(line.to_string());
    }

    let mut head: Vec<String> = Vec::new();
    if n_pass > 0 {
        head.push(format!("({} passing tests stripped)", n_pass));
    }
    if n_skip > 0 {
        head.push(format!("({} skipped tests stripped)", n_skip));
    }
    if framework_frames_skipped > 0 {
        head.push(format!(
            "({} testing/runtime framework frames stripped)",
            framework_frames_skipped
        ));
    }
    head.extend(kept);
    head.join("\n").trim().to_string()
}

// ---------- gradle ----------

static GRADLE_BANNER_KEYWORDS: &[&str] = &[
    "Welcome to Gradle",
    "Here are the highlights",
    " - ",
    "For more details see",
    "To honour the JVM",
    "Daemon will be stopped",
    "actionable task",
];
static GRADLE_TASK_PROGRESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^>\s+Task\s+:\S+\s*(?:UP-TO-DATE|NO-SOURCE|FROM-CACHE)?\s*$").unwrap()
});
static GRADLE_TASK_FAILED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^>\s+Task\s+:\S+\s+FAILED").unwrap());

pub fn digest_gradle(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut n_task_ok = 0usize;
    let mut in_banner = false;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        // banner / boilerplate
        if GRADLE_BANNER_KEYWORDS.iter().any(|kw| line.contains(kw)) {
            in_banner = true;
            continue;
        }
        if in_banner && line.starts_with("- ") {
            continue;
        }
        in_banner = false;

        if GRADLE_TASK_FAILED.is_match(line) {
            kept.push(line.to_string());
            continue;
        }
        if GRADLE_TASK_PROGRESS.is_match(line) {
            n_task_ok += 1;
            continue;
        }
        kept.push(line.to_string());
    }

    let mut head: Vec<String> = Vec::new();
    if n_task_ok > 0 {
        head.push(format!("({} gradle tasks ok, stripped)", n_task_ok));
    }
    head.extend(kept);
    head.join("\n").trim().to_string()
}

// ---------- ruff ----------

static RUFF_LINE_PARSE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^([^:\s]+\.py):(\d+):(\d+):\s+([A-Z]+\d+)\s+(.+)$").unwrap());

pub fn digest_ruff(text: &str) -> String {
    let text = strip_ansi(text);
    let mut by_code: HashMap<String, Vec<(String, usize, String)>> = HashMap::new();
    let mut other: Vec<String> = Vec::new();
    let mut total = 0usize;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = RUFF_LINE_PARSE.captures(line) {
            total += 1;
            let file = caps.get(1).unwrap().as_str().to_string();
            let lineno: usize = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            let code = caps.get(4).unwrap().as_str().to_string();
            let msg = caps.get(5).unwrap().as_str().trim().to_string();
            by_code.entry(code).or_default().push((file, lineno, msg));
        } else {
            other.push(line.to_string());
        }
    }

    let mut out: Vec<String> = Vec::new();
    if total > 0 {
        out.push(format!(
            "=== ruff: {} violations, {} unique rules ===",
            total,
            by_code.len()
        ));
        let mut codes: Vec<(&String, &Vec<(String, usize, String)>)> = by_code.iter().collect();
        codes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (code, items) in codes {
            out.push(format!("[{}] {} violations:", code, items.len()));
            for (file, ln, msg) in items.iter().take(3) {
                out.push(format!("  {}:{}: {}", file, ln, msg));
            }
            if items.len() > 3 {
                out.push(format!("  ... +{} more", items.len() - 3));
            }
        }
    }
    for line in other {
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

// ---------- mypy ----------

static MYPY_INSTALL_PROGRESS: &[&str] = &["Downloading", "Downloaded", "Installed", "Resolved"];

pub fn digest_mypy(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut install_lines = 0usize;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        if MYPY_INSTALL_PROGRESS
            .iter()
            .any(|kw| line.trim_start().starts_with(kw))
        {
            install_lines += 1;
            continue;
        }
        kept.push(line.to_string());
    }
    if install_lines > 0 {
        kept.insert(
            0,
            format!("({} install/progress lines stripped)", install_lines),
        );
    }
    kept.join("\n").trim().to_string()
}

// ---------- compiler (clang / gcc / cmake / make / swiftc) ----------

// Compiler progress / step lines to strip.
static COMPILER_PROGRESS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"^\[\s*\d+%\]\s+(Built target|Building|Linking|Generating)",
        r"^\[\d+/\d+\]\s+\S+", // swift / ninja-style step
        r"^make\[\d+\]: Entering directory",
        r"^make\[\d+\]: Leaving directory",
        r"^make\[\d+\]: Nothing to be done",
        r"^Building for debugging",
        r"^Build complete!",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});
// Source-pointer / context lines emitted under a diagnostic, e.g.
//   "    4 |     printf(...)"
//   "      |                ~^"
// Keep at most 2 such lines per error to retain meaningful context without bloat.
static COMPILER_CONTEXT_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\d+\s*\|").unwrap());
static COMPILER_CARET_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s+\|.*[\^~]").unwrap());

pub fn digest_compiler(text: &str) -> String {
    let text = strip_ansi(text);
    let mut kept: Vec<String> = Vec::new();
    let mut progress_stripped = 0usize;
    let mut after_diag_ctx_seen = 0usize;
    let mut last_was_diag = false;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            last_was_diag = false;
            after_diag_ctx_seen = 0;
            continue;
        }
        if COMPILER_PROGRESS.iter().any(|p| p.is_match(line)) {
            progress_stripped += 1;
            continue;
        }
        if COMPILER_DIAG.is_match(line) {
            kept.push(line.to_string());
            last_was_diag = true;
            after_diag_ctx_seen = 0;
            continue;
        }
        // Context / caret lines under a diagnostic — keep up to 2 lines.
        if last_was_diag
            && (COMPILER_CONTEXT_LINE.is_match(line) || COMPILER_CARET_LINE.is_match(line))
        {
            if after_diag_ctx_seen < 2 {
                kept.push(line.to_string());
                after_diag_ctx_seen += 1;
            }
            continue;
        }
        // Any "N errors generated", "BUILD FAILED", "make: *** ... Error N", etc.
        kept.push(line.to_string());
        last_was_diag = false;
        after_diag_ctx_seen = 0;
    }

    if progress_stripped > 0 {
        kept.insert(
            0,
            format!(
                "({} compile/link progress lines stripped)",
                progress_stripped
            ),
        );
    }
    kept.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_strips_compile_lines() {
        let input = "   Compiling foo v1.0\n   Compiling bar v2.0\n    Finished `release` profile [optimized] target(s) in 5s\n";
        let out = digest_cargo(input);
        assert!(out.contains("Finished"));
        assert!(!out.contains("Compiling foo"));
    }

    #[test]
    fn cargo_test_collapses_passing() {
        let input = "running 3 tests\ntest a ... ok\ntest b ... ok\ntest c ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored\n";
        let out = digest_cargo(input);
        assert!(out.contains("3 passing tests stripped"));
        assert!(out.contains("test result: ok"));
    }

    #[test]
    fn tsc_groups_by_code() {
        let input = "src/a.ts(1,1): error TS2322: Type 'string' is not assignable to type 'number'.\n\
                     src/a.ts(2,1): error TS2322: Type 'number' is not assignable to type 'string'.\n\
                     src/b.ts(3,1): error TS2345: Argument of type 'string'.\n";
        let out = digest_tsc(input);
        assert!(out.contains("3 total"));
        assert!(out.contains("[TS2322] 2 errors"));
        assert!(out.contains("[TS2345] 1 errors"));
    }

    #[test]
    fn ci_collapses_ok_groups_and_keeps_failed() {
        let input = "##[group]Setup\nFoo\nBar\n##[endgroup]\n\
                     ##[group]Build\nworking ...\nerror: build failed\n##[endgroup]\n";
        let out = digest_ci(input);
        assert!(out.contains("[ok] Setup"));
        assert!(out.contains("FAILED GROUP: Build"));
        assert!(out.contains("error: build failed"));
    }

    #[test]
    fn ci_strips_gha_timestamp_prefix() {
        let input = "job1\tstep1\t2024-01-01T00:00:00.0000000Z hello world\n";
        let out = digest_ci(input);
        assert!(out.contains("hello world"));
        assert!(!out.contains("2024-01-01"));
    }

    #[test]
    fn detect_picks_cargo() {
        assert_eq!(detect("   Compiling foo v1.0\n"), Format::Cargo);
    }

    #[test]
    fn detect_picks_ci() {
        assert_eq!(detect("##[group]Setup\nFoo\n"), Format::Ci);
    }

    #[test]
    fn detect_picks_tsc() {
        assert_eq!(detect("src/a.ts(1,1): error TS2322: Type ..."), Format::Tsc);
    }

    #[test]
    fn detect_unknown_returns_text() {
        let input = "just some random text\nnot any tool";
        assert_eq!(detect(input), Format::Unknown);
        // digest with Unknown preserves input
        let out = digest(input, Format::Unknown);
        assert!(out.contains("just some random text"));
    }

    #[test]
    fn cargo_strips_ansi_escape_codes() {
        let input = "\x1b[31m   Compiling foo v1.0\x1b[0m\n\x1b[32m    Finished `release` profile in 5s\x1b[0m\n";
        let out = digest_cargo(input);
        assert!(!out.contains("\x1b"), "ANSI not stripped: {out:?}");
        assert!(out.contains("Finished"));
        assert!(!out.contains("Compiling foo"));
    }

    #[test]
    fn pnpm_collapses_deprecation_warnings() {
        let input = "warn deprecated foo@1.0\nwarn deprecated foo@1.0\nwarn deprecated bar@2.0\n+ react 18.0\n";
        let out = digest_pnpm(input);
        assert!(out.contains("deprecated packages"));
        assert!(out.contains("foo"));
        assert!(out.contains("bar"));
        assert!(out.contains("react 18.0"));
    }

    #[test]
    fn ci_handles_unclosed_group() {
        let input = "##[group]Unfinished\nworking\nworking more\nerror: build failed mid-stream\n";
        let out = digest_ci(input);
        assert!(out.contains("Unfinished"));
        assert!(out.contains("error: build failed"));
        assert!(out.contains("still open"));
    }

    #[test]
    fn digest_via_dispatch_picks_handler() {
        let cargo_input = "   Compiling foo v1.0\n    Finished `release` in 5s\n";
        let out = digest(cargo_input, Format::Cargo);
        assert!(out.contains("Finished"));
        assert!(!out.contains("Compiling foo"));
    }

    #[test]
    fn detect_picks_pnpm_via_npm_warn() {
        assert_eq!(detect("npm warn deprecated x@1\n"), Format::Pnpm);
    }

    #[test]
    fn detect_picks_pytest() {
        let s = "test session starts\ntest_x.py::test_y FAILED\nFAILED test_x.py::test_y\n=== 1 failed in 0.01s ===\n";
        assert_eq!(detect(s), Format::Pytest);
    }

    #[test]
    fn ci_handles_real_gha_log_format() {
        // simulates gh run view --log-failed prefix
        let input = "PR - x\trun the build\t\u{feff}2026-05-14T09:20:49.4757960Z ##[group]Run set +e\n\
                     PR - x\trun the build\t2026-05-14T09:20:49.4758449Z working\n\
                     PR - x\trun the build\t2026-05-14T09:20:49.4759411Z ##[endgroup]\n\
                     PR - x\trun the build\t2026-05-14T09:56:05.7508664Z ##[error]Process completed with exit code 1.\n";
        let out = digest_ci(input);
        assert!(out.contains("[ok] Run set +e"));
        assert!(out.contains("##[error]Process completed"));
        assert!(!out.contains("2026-05-14T"));
        assert!(!out.contains("PR - x"));
    }

    #[test]
    fn format_parse_aliases() {
        assert_eq!(Format::parse("rust"), Some(Format::Cargo));
        assert_eq!(Format::parse("npm"), Some(Format::Pnpm));
        assert_eq!(Format::parse("yarn"), Some(Format::Pnpm));
        assert_eq!(Format::parse("bun"), Some(Format::Pnpm));
        assert_eq!(Format::parse("github-actions"), Some(Format::Ci));
        assert_eq!(Format::parse("typescript"), Some(Format::Tsc));
        assert_eq!(Format::parse("python"), Some(Format::Pytest));
        assert_eq!(Format::parse("go-test"), Some(Format::GoTest));
        assert_eq!(Format::parse("gradle"), Some(Format::Gradle));
        assert_eq!(Format::parse("ruff"), Some(Format::Ruff));
        assert_eq!(Format::parse("mypy"), Some(Format::Mypy));
        assert_eq!(Format::parse("clang"), Some(Format::Compiler));
        assert_eq!(Format::parse("swift"), Some(Format::Compiler));
        assert_eq!(Format::parse("nonexistent"), None);
    }

    // ----- go test handler -----

    #[test]
    fn go_strips_run_and_counts_pass() {
        let input = "=== RUN   TestA\n--- PASS: TestA (0.00s)\n=== RUN   TestB\n    foo.go:1: oops\n--- FAIL: TestB (0.00s)\nFAIL\tpkg\t0.1s\nFAIL\n";
        let out = digest_go_test(input);
        assert!(out.contains("1 passing tests stripped"));
        assert!(out.contains("--- FAIL: TestB"));
        assert!(out.contains("foo.go:1: oops"));
        assert!(!out.contains("=== RUN"));
    }

    #[test]
    fn go_strips_framework_stack_frames() {
        let input = "panic: divide by zero\ngoroutine 6 [running]:\ntesting.tRunner.func1(0x123)\n\t/opt/homebrew/Cellar/go/1.25/libexec/src/testing/testing.go:1872 +0x190\nmypkg.Foo()\n\t/tmp/proj/foo.go:5 +0x20\nFAIL\tpkg\t0.1s\n";
        let out = digest_go_test(input);
        assert!(out.contains("panic: divide by zero"));
        assert!(out.contains("mypkg.Foo()"));
        assert!(out.contains("/tmp/proj/foo.go:5"));
        assert!(!out.contains("testing.tRunner"));
    }

    #[test]
    fn detect_picks_go_test() {
        let input = "=== RUN   TestFoo\n--- PASS: TestFoo (0.00s)\n";
        assert_eq!(detect(input), Format::GoTest);
    }

    // ----- gradle handler -----

    #[test]
    fn gradle_strips_banner_and_progress_keeps_failures() {
        let input = "Welcome to Gradle 8.14!\n\
                     \n\
                     Here are the highlights of this release:\n\
                     - foo\n\
                     - bar\n\
                     \n\
                     > Task :compileJava\n\
                     > Task :classes\n\
                     > Task :test FAILED\n\
                     \n\
                     BarTest > testAddFail FAILED\n\
                     \n\
                     BUILD FAILED in 4s\n";
        let out = digest_gradle(input);
        assert!(!out.contains("Welcome to Gradle"));
        assert!(out.contains("2 gradle tasks ok"));
        assert!(out.contains("> Task :test FAILED"));
        assert!(out.contains("BarTest > testAddFail FAILED"));
        assert!(out.contains("BUILD FAILED"));
    }

    #[test]
    fn detect_picks_gradle() {
        assert_eq!(detect("> Task :compileJava\n"), Format::Gradle);
    }

    // ----- ruff handler -----

    #[test]
    fn ruff_groups_by_rule_code() {
        let input = "app.py:1:8: F401 [*] `os` imported but unused\n\
                     app.py:2:8: F401 [*] `sys` imported but unused\n\
                     app.py:3:8: F401 [*] `json` imported but unused\n\
                     app.py:5:5: F841 Local variable `x` is assigned but never used\n\
                     app.py:7:1: E741 Ambiguous variable name: `l`\n\
                     Found 5 errors.\n";
        let out = digest_ruff(input);
        assert!(out.contains("ruff: 5 violations, 3 unique rules"));
        assert!(out.contains("[F401] 3 violations"));
        assert!(out.contains("[F841] 1 violations"));
        assert!(out.contains("[E741] 1 violations"));
        assert!(out.contains("Found 5 errors"));
    }

    #[test]
    fn detect_picks_ruff() {
        let input = "app.py:1:8: F401 `os` imported but unused\n";
        assert_eq!(detect(input), Format::Ruff);
    }

    // ----- mypy handler -----

    #[test]
    fn mypy_strips_install_progress() {
        let input = "Downloading mypy (13MB)\n\
                     Downloaded mypy\n\
                     Installed 6 packages in 200ms\n\
                     types.py:5: error: incompatible type\n\
                     types.py:8: error: invalid return\n\
                     Found 2 errors in 1 file\n";
        let out = digest_mypy(input);
        assert!(out.contains("install/progress lines stripped"));
        assert!(out.contains("types.py:5: error: incompatible type"));
        assert!(out.contains("types.py:8: error: invalid return"));
        assert!(out.contains("Found 2 errors"));
        assert!(!out.contains("Downloading"));
        assert!(!out.contains("Downloaded"));
    }

    #[test]
    fn detect_picks_mypy() {
        let input = "types.py:5: error: incompatible type\n";
        assert_eq!(detect(input), Format::Mypy);
    }

    // ----- compiler handler -----

    #[test]
    fn compiler_strips_build_progress() {
        let input = "[ 50%] Building CXX object foo.cpp.o\n\
                     [100%] Linking CXX executable demo\n\
                     /tmp/proj/main.cpp:4:18: error: use of undeclared identifier 'y'\n\
                         4 |     std::cout << y << std::endl;\n\
                           |                  ^\n\
                     1 error generated.\n";
        let out = digest_compiler(input);
        assert!(out.contains("compile/link progress lines stripped"));
        assert!(out.contains("main.cpp:4:18: error: use of undeclared identifier"));
        assert!(out.contains("std::cout << y"));
        assert!(out.contains("1 error generated"));
        assert!(!out.contains("Building CXX"));
        assert!(!out.contains("Linking CXX"));
    }

    #[test]
    fn compiler_keeps_caret_context() {
        let input = "hello.c:5:12: error: undeclared function\n\
                         5 |     return undefined_function();\n\
                           |            ^\n";
        let out = digest_compiler(input);
        assert!(out.contains("hello.c:5:12: error"));
        assert!(out.contains("return undefined_function()"));
        assert!(out.contains("^"));
    }

    #[test]
    fn detect_picks_compiler() {
        let input = "main.cpp:4:18: error: use of undeclared identifier 'y'\n";
        assert_eq!(detect(input), Format::Compiler);
    }

    #[test]
    fn detect_picks_compiler_from_swift_steps() {
        let input = "[1/4] Compiling DigestDemo DigestDemo.swift\n[2/4] Emitting module\n";
        assert_eq!(detect(input), Format::Compiler);
    }
}
