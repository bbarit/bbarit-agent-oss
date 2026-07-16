use once_cell::sync::Lazy;
use regex::Regex;

use crate::types::{Chunk, SearchResult};

static SCP_GIT_URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\w.-]+@[\w.-]+:").unwrap());

const GIT_URL_SCHEMES: &[&str] = &[
    "https://",
    "http://",
    "ssh://",
    "git://",
    "git+ssh://",
    "file://",
];

pub fn is_git_url(path: &str) -> bool {
    if GIT_URL_SCHEMES.iter().any(|s| path.starts_with(s)) {
        return true;
    }
    if let Some(mat) = SCP_GIT_URL_RE.find(path) {
        let rest = &path[mat.end()..];
        return !rest.starts_with('/');
    }
    false
}

pub fn resolve_chunk<'a>(chunks: &'a [Chunk], file_path: &str, line: usize) -> Option<&'a Chunk> {
    let mut fallback = None;
    for chunk in chunks {
        if chunk.file_path == file_path && chunk.start_line <= line && line <= chunk.end_line {
            if line < chunk.end_line {
                return Some(chunk);
            }
            if fallback.is_none() {
                fallback = Some(chunk);
            }
        }
    }
    fallback
}

pub fn format_results(header: &str, results: &[SearchResult]) -> String {
    let mut lines = vec![header.to_string(), String::new()];
    for (i, r) in results.iter().enumerate() {
        lines.push(format!(
            "## {}. {}  [score={:.3}]",
            i + 1,
            r.chunk.location(),
            r.score
        ));
        if !r.match_lines.is_empty() {
            for ml in &r.match_lines {
                lines.push(format!("  L{}: {}", ml.line, ml.content));
            }
        } else {
            lines.push("```".to_string());
            lines.push(r.chunk.content.trim().to_string());
            lines.push("```".to_string());
        }
        lines.push(String::new());
    }
    lines.join("\n")
}
