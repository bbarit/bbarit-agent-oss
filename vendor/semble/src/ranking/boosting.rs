use std::collections::{HashMap, HashSet};
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::tokens::split_identifier;
use crate::types::Chunk;

static SYMBOL_QUERY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"^(?:",
        r"[A-Za-z_][A-Za-z0-9_]*(?:(?:::|\\|->|\.)[A-Za-z_][A-Za-z0-9_]*)+",
        r"|_[A-Za-z0-9_]*",
        r"|[A-Za-z][A-Za-z0-9]*[A-Z_][A-Za-z0-9_]*",
        r"|[A-Z][A-Za-z0-9]*",
        r")$",
    ))
    .unwrap()
});

static EMBEDDED_SYMBOL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"\b(?:",
        r"[A-Z][a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]*",
        r"|[a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]+",
        r")\b",
    ))
    .unwrap()
});

const EMBEDDED_STEM_MIN_LEN: usize = 4;
const EMBEDDED_SYMBOL_BOOST_SCALE: f64 = 0.5;

const DEFINITION_KEYWORDS: &[&str] = &[
    "class",
    "module",
    "defmodule",
    "def",
    "interface",
    "struct",
    "enum",
    "trait",
    "type",
    "func",
    "function",
    "object",
    "abstract class",
    "data class",
    "fn",
    "fun",
    "package",
    "namespace",
    "protocol",
    "record",
    "typedef",
];

const SQL_DEFINITION_KEYWORDS: &[&str] = &[
    "CREATE TABLE",
    "CREATE VIEW",
    "CREATE PROCEDURE",
    "CREATE FUNCTION",
];

static DEFINITION_KEYWORD_BODY: Lazy<String> = Lazy::new(|| {
    DEFINITION_KEYWORDS
        .iter()
        .map(|kw| regex::escape(kw))
        .collect::<Vec<_>>()
        .join("|")
});

static SQL_KEYWORD_BODY: Lazy<String> = Lazy::new(|| {
    SQL_DEFINITION_KEYWORDS
        .iter()
        .map(|kw| regex::escape(kw))
        .collect::<Vec<_>>()
        .join("|")
});

const DEFINITION_BOOST_MULTIPLIER: f64 = 3.0;
const STEM_BOOST_MULTIPLIER: f64 = 1.0;
const FILE_COHERENCE_BOOST_FRAC: f64 = 0.2;

static STOPWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    "a an and are as at be by do does for from has have how if in is it not of on or the to was \
     what when where which who why with"
        .split_whitespace()
        .collect()
});

pub fn is_symbol_query(query: &str) -> bool {
    SYMBOL_QUERY_RE.is_match(query.trim())
}

pub fn apply_query_boost(scores: &mut HashMap<usize, f64>, query: &str, chunks: &[Chunk]) {
    if scores.is_empty() {
        return;
    }
    let max_score = scores.values().cloned().fold(f64::NEG_INFINITY, f64::max);

    if is_symbol_query(query) {
        boost_symbol_definitions(scores, query, max_score, chunks);
    } else {
        boost_stem_matches(scores, query, max_score, chunks);
        boost_embedded_symbols(scores, query, max_score, chunks);
    }
}

pub fn boost_multi_chunk_files(scores: &mut HashMap<usize, f64>, chunks: &[Chunk]) {
    if scores.is_empty() {
        return;
    }
    let max_score = scores.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    if max_score == 0.0 {
        return;
    }

    let mut file_sum: HashMap<&str, f64> = HashMap::new();
    let mut best_chunk: HashMap<&str, usize> = HashMap::new();

    for (&idx, &score) in scores.iter() {
        let fp = chunks[idx].file_path.as_str();
        *file_sum.entry(fp).or_default() += score;
        let is_best = best_chunk.get(fp).is_none_or(|&prev| score > scores[&prev]);
        if is_best {
            best_chunk.insert(fp, idx);
        }
    }

    let max_file_sum = file_sum.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    let boost_unit = max_score * FILE_COHERENCE_BOOST_FRAC;

    for (fp, &idx) in &best_chunk {
        if let Some(&fsum) = file_sum.get(fp) {
            *scores.entry(idx).or_default() += boost_unit * fsum / max_file_sum;
        }
    }
}

fn extract_symbol_name(query: &str) -> &str {
    let q = query.trim();
    for sep in &["::", "\\", "->", "."] {
        if let Some(pos) = q.rfind(sep) {
            return &q[pos + sep.len()..];
        }
    }
    q
}

fn chunk_defines_symbol(chunk: &Chunk, symbol_name: &str) -> bool {
    let escaped = regex::escape(symbol_name);
    let ns_prefix = r"(?:[A-Za-z_][A-Za-z0-9_]*(?:\.|::))*";
    let tail = format!(r"\s+{ns_prefix}{escaped}(?:\s|[<({{\[;:]|$)");

    let general = format!(r"(?m)(?:^|\s)(?:{}){tail}", &*DEFINITION_KEYWORD_BODY);
    let sql = format!(r"(?mi)(?:^|\s)(?:{}){tail}", &*SQL_KEYWORD_BODY);

    Regex::new(&general).is_ok_and(|re| re.is_match(&chunk.content))
        || Regex::new(&sql).is_ok_and(|re| re.is_match(&chunk.content))
}

fn stem_matches(stem: &str, name: &str) -> bool {
    let stem_norm = stem.replace('_', "");
    stem == name
        || stem_norm == name
        || stem.trim_end_matches('s') == name
        || stem_norm.trim_end_matches('s') == name
}

fn definition_tier(chunk: &Chunk, names: &HashSet<String>, boost_unit: f64) -> Option<f64> {
    if !names.iter().any(|name| chunk_defines_symbol(chunk, name)) {
        return None;
    }
    let stem = Path::new(&chunk.file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let multiplier = if names
        .iter()
        .any(|name| stem_matches(&stem, &name.to_lowercase()))
    {
        1.5
    } else {
        1.0
    };
    Some(boost_unit * multiplier)
}

fn scan_non_candidates(
    scores: &mut HashMap<usize, f64>,
    names: &HashSet<String>,
    boost_unit: f64,
    chunks: &[Chunk],
    stem_ok: impl Fn(&str) -> bool,
) {
    for (idx, chunk) in chunks.iter().enumerate() {
        if scores.contains_key(&idx) {
            continue;
        }
        let stem = Path::new(&chunk.file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !stem_ok(&stem) {
            continue;
        }
        if let Some(tier) = definition_tier(chunk, names, boost_unit) {
            scores.insert(idx, tier);
        }
    }
}

fn boost_symbol_definitions(
    scores: &mut HashMap<usize, f64>,
    query: &str,
    max_score: f64,
    chunks: &[Chunk],
) {
    let symbol_name = extract_symbol_name(query);
    let mut names = HashSet::new();
    names.insert(symbol_name.to_string());
    if symbol_name != query.trim() {
        names.insert(query.trim().to_string());
    }

    let boost_unit = max_score * DEFINITION_BOOST_MULTIPLIER;

    let existing: Vec<usize> = scores.keys().cloned().collect();
    for idx in existing {
        if let Some(tier) = definition_tier(&chunks[idx], &names, boost_unit) {
            *scores.entry(idx).or_default() += tier;
        }
    }

    let sym_lower = symbol_name.to_lowercase();
    scan_non_candidates(scores, &names, boost_unit, chunks, |stem| {
        stem_matches(stem, &sym_lower)
    });
}

fn boost_embedded_symbols(
    scores: &mut HashMap<usize, f64>,
    query: &str,
    max_score: f64,
    chunks: &[Chunk],
) {
    let names: HashSet<String> = EMBEDDED_SYMBOL_RE
        .find_iter(query)
        .map(|m| m.as_str().to_string())
        .collect();
    if names.is_empty() {
        return;
    }

    let boost_unit = max_score * DEFINITION_BOOST_MULTIPLIER * EMBEDDED_SYMBOL_BOOST_SCALE;

    let existing: Vec<usize> = scores.keys().cloned().collect();
    for idx in existing {
        if let Some(tier) = definition_tier(&chunks[idx], &names, boost_unit) {
            *scores.entry(idx).or_default() += tier;
        }
    }

    let symbols_lower: HashSet<String> = names.iter().map(|s| s.to_lowercase()).collect();
    for (idx, chunk) in chunks.iter().enumerate() {
        if scores.contains_key(&idx) {
            continue;
        }
        let stem = Path::new(&chunk.file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let stem_norm = stem.replace('_', "");
        let matched = symbols_lower.iter().any(|sym| {
            stem == *sym
                || stem_norm == *sym
                || (stem.len() >= EMBEDDED_STEM_MIN_LEN && sym.starts_with(&stem))
                || (stem_norm.len() >= EMBEDDED_STEM_MIN_LEN && sym.starts_with(&stem_norm))
        });
        if !matched {
            continue;
        }
        if let Some(tier) = definition_tier(chunk, &names, boost_unit) {
            scores.insert(idx, tier);
        }
    }
}

fn count_keyword_matches(keywords: &HashSet<String>, parts: &HashSet<String>) -> usize {
    let exact: HashSet<&String> = keywords.intersection(parts).collect();
    if exact.len() == keywords.len() {
        return exact.len();
    }
    let mut n = exact.len();
    for kw in keywords {
        if exact.contains(kw) {
            continue;
        }
        for part in parts {
            let (shorter, longer) = if kw.len() <= part.len() {
                (kw.as_str(), part.as_str())
            } else {
                (part.as_str(), kw.as_str())
            };
            if shorter.len() >= 3 && longer.starts_with(shorter) {
                n += 1;
                break;
            }
        }
    }
    n
}

fn boost_stem_matches(
    scores: &mut HashMap<usize, f64>,
    query: &str,
    max_score: f64,
    chunks: &[Chunk],
) {
    static WORD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*").unwrap());

    let keywords: HashSet<String> = WORD_RE
        .find_iter(query)
        .map(|m| m.as_str().to_lowercase())
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(w.as_str()))
        .collect();
    if keywords.is_empty() {
        return;
    }

    let boost = max_score * STEM_BOOST_MULTIPLIER;
    let mut path_cache: HashMap<String, HashSet<String>> = HashMap::new();

    let existing: Vec<usize> = scores.keys().cloned().collect();
    for idx in existing {
        let chunk = &chunks[idx];
        let parts = path_cache
            .entry(chunk.file_path.clone())
            .or_insert_with(|| {
                let path = Path::new(&chunk.file_path);
                let mut parts: HashSet<String> =
                    split_identifier(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
                        .into_iter()
                        .collect();
                if let Some(parent_name) = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                {
                    if parent_name != "." && parent_name != "/" && parent_name != ".." {
                        parts.extend(split_identifier(parent_name));
                    }
                }
                parts
            })
            .clone();

        let n = count_keyword_matches(&keywords, &parts);
        if n > 0 {
            let match_ratio = n as f64 / keywords.len() as f64;
            if match_ratio >= 0.10 {
                *scores.entry(idx).or_default() += boost * match_ratio;
            }
        }
    }
}
