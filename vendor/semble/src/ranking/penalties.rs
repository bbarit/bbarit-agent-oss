use std::collections::HashMap;
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::types::Chunk;

static TEST_FILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?:^|/)",
        r"(?:",
        r"test_[^/]*\.py",
        r"|[^/]*_test\.py",
        r"|[^/]*_test\.go",
        r"|[^/]*Tests?\.java",
        r"|[^/]*Test\.php",
        r"|[^/]*_spec\.rb",
        r"|[^/]*_test\.rb",
        r"|[^/]*\.test\.[jt]sx?",
        r"|[^/]*\.spec\.[jt]sx?",
        r"|[^/]*Tests?\.kt",
        r"|[^/]*Spec\.kt",
        r"|[^/]*Tests?\.swift",
        r"|[^/]*Spec\.swift",
        r"|[^/]*Tests?\.cs",
        r"|test_[^/]*\.cpp",
        r"|[^/]*_test\.cpp",
        r"|test_[^/]*\.c",
        r"|[^/]*_test\.c",
        r"|[^/]*Spec\.scala",
        r"|[^/]*Suite\.scala",
        r"|[^/]*Test\.scala",
        r"|[^/]*_test\.dart",
        r"|test_[^/]*\.dart",
        r"|[^/]*_spec\.lua",
        r"|[^/]*_test\.lua",
        r"|test_[^/]*\.lua",
        r"|test_helpers?[^/]*\.\w+",
        r")$",
    ))
    .unwrap()
});

static TEST_DIR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|/)(?:tests?|__tests__|spec|testing)(?:/|$)").unwrap());

static COMPAT_DIR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|/)(?:compat|_compat|legacy)(?:/|$)").unwrap());

static EXAMPLES_DIR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|/)(?:_?examples?|docs?_src)(?:/|$)").unwrap());

static TYPE_DEFS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\.d\.ts$").unwrap());

const STRONG_PENALTY: f64 = 0.3;
const MODERATE_PENALTY: f64 = 0.5;
const MILD_PENALTY: f64 = 0.7;

const REEXPORT_FILENAMES: &[&str] = &["__init__.py", "package-info.java"];

const FILE_SATURATION_THRESHOLD: usize = 1;
const FILE_SATURATION_DECAY: f64 = 0.5;

pub fn rerank_topk(
    scores: &HashMap<usize, f64>,
    chunks: &[Chunk],
    top_k: usize,
    penalise_paths: bool,
) -> Vec<(usize, f64)> {
    if scores.is_empty() {
        return Vec::new();
    }

    let mut penalty_cache: HashMap<&str, f64> = HashMap::new();
    let mut penalised: Vec<(usize, f64)> = Vec::with_capacity(scores.len());

    for (&idx, &score) in scores {
        let penalty = if penalise_paths {
            let fp = chunks[idx].file_path.as_str();
            *penalty_cache
                .entry(fp)
                .or_insert_with(|| file_path_penalty(fp))
        } else {
            1.0
        };
        penalised.push((idx, score * penalty));
    }

    penalised.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut file_selected: HashMap<&str, usize> = HashMap::new();
    let mut selected: Vec<(f64, usize)> = Vec::new();
    let mut min_selected = f64::INFINITY;

    for &(idx, pen_score) in &penalised {
        if selected.len() >= top_k && pen_score <= min_selected {
            break;
        }

        let fp = chunks[idx].file_path.as_str();
        let already = *file_selected.get(fp).unwrap_or(&0);
        let mut eff_score = pen_score;

        if already >= FILE_SATURATION_THRESHOLD {
            let excess = (already - FILE_SATURATION_THRESHOLD + 1) as i32;
            eff_score *= FILE_SATURATION_DECAY.powi(excess);
        }

        selected.push((eff_score, idx));
        *file_selected.entry(fp).or_default() += 1;

        if selected.len() >= top_k {
            min_selected = selected
                .iter()
                .map(|(s, _)| *s)
                .fold(f64::INFINITY, f64::min);
        }
    }

    selected.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    selected
        .into_iter()
        .take(top_k)
        .map(|(score, idx)| (idx, score))
        .collect()
}

fn file_path_penalty(file_path: &str) -> f64 {
    let normalised = file_path.replace('\\', "/");
    let mut penalty = 1.0;

    if TEST_FILE_RE.is_match(&normalised) || TEST_DIR_RE.is_match(&normalised) {
        penalty *= STRONG_PENALTY;
    }
    if let Some(name) = Path::new(file_path).file_name().and_then(|n| n.to_str()) {
        if REEXPORT_FILENAMES.contains(&name) {
            penalty *= MODERATE_PENALTY;
        }
    }
    if COMPAT_DIR_RE.is_match(&normalised) {
        penalty *= STRONG_PENALTY;
    }
    if EXAMPLES_DIR_RE.is_match(&normalised) {
        penalty *= STRONG_PENALTY;
    }
    if TYPE_DEFS_RE.is_match(&normalised) {
        penalty *= MILD_PENALTY;
    }

    penalty
}
