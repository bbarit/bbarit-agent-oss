use std::collections::HashMap;

use crate::bm25::Bm25Index;
use crate::encoder::{SemanticIndex, StaticEncoder};
use crate::graph::DependencyGraph;
use crate::ranking::{apply_query_boost, boost_multi_chunk_files, rerank_topk, resolve_alpha};
use crate::tokens::tokenize;
use crate::types::{Chunk, MatchLine, SearchResult};

const RRF_K: f64 = 60.0;
const MIN_SCORE_RATIO: f64 = 0.12;

fn rrf_scores(scores: &HashMap<usize, f64>) -> HashMap<usize, f64> {
    if scores.is_empty() {
        return HashMap::new();
    }
    let mut ranked: Vec<(usize, f64)> = scores.iter().map(|(&k, &v)| (k, v)).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
        .iter()
        .enumerate()
        .map(|(rank, &(idx, _))| (idx, 1.0 / (RRF_K + rank as f64 + 1.0)))
        .collect()
}

fn selector_to_mask(selector: Option<&[usize]>, size: usize) -> Option<Vec<bool>> {
    let indices = selector?;
    let mut mask = vec![false; size];
    for &idx in indices {
        if idx < size {
            mask[idx] = true;
        }
    }
    Some(mask)
}

fn find_match_lines(chunk: &Chunk, query: &str) -> Vec<MatchLine> {
    let query_lower = query.to_lowercase();
    let keywords: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() >= 2)
        .collect();
    if keywords.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for (i, line) in chunk.content.lines().enumerate() {
        let line_lower = line.to_lowercase();
        if keywords.iter().any(|kw| line_lower.contains(kw)) {
            matches.push(MatchLine {
                line: chunk.start_line + i,
                content: line.trim().to_string(),
            });
        }
    }
    matches
}

fn boost_sibling_chunks(scores: &mut HashMap<usize, f64>, chunks: &[Chunk], query: &str) {
    let keywords: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect();
    if keywords.is_empty() {
        return;
    }

    let mut file_has_match: HashMap<&str, f64> = HashMap::new();
    for (idx, chunk) in chunks.iter().enumerate() {
        let content_lower = chunk.content.to_lowercase();
        if keywords.iter().any(|kw| content_lower.contains(kw)) {
            let score = scores.get(&idx).copied().unwrap_or(0.001);
            let fp = chunk.file_path.as_str();
            let entry = file_has_match.entry(fp).or_insert(0.0);
            if score > *entry {
                *entry = score;
            }
        }
    }

    for (idx, chunk) in chunks.iter().enumerate() {
        let content_lower = chunk.content.to_lowercase();
        let match_count = keywords
            .iter()
            .filter(|kw| content_lower.contains(kw.as_str()))
            .count();
        if match_count == 0 {
            continue;
        }
        if let Some(existing) = scores.get_mut(&idx) {
            let boost = *existing * 0.3 * match_count as f64;
            *existing += boost;
        } else if let Some(&file_score) = file_has_match.get(chunk.file_path.as_str()) {
            scores.insert(idx, file_score * (0.8 + 0.2 * match_count as f64));
        }
    }
}

fn filter_low_scores(results: Vec<SearchResult>) -> Vec<SearchResult> {
    if results.len() <= 1 {
        return results;
    }
    let top_score = results[0].score;
    if top_score <= 0.0 {
        return Vec::new();
    }
    let min = top_score * MIN_SCORE_RATIO;
    results.into_iter().filter(|r| r.score >= min).collect()
}

fn boost_from_graph(scores: &mut HashMap<usize, f64>, chunks: &[Chunk], graph: &DependencyGraph) {
    if scores.is_empty() {
        return;
    }
    let max_score = scores.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    if max_score <= 0.0 {
        return;
    }

    let mut top_files: Vec<(&str, f64)> = Vec::new();
    for (&idx, &score) in scores.iter() {
        let fp = chunks[idx].file_path.as_str();
        if score >= max_score * 0.5 {
            top_files.push((fp, score));
        }
    }

    let boost = max_score * 0.3;
    for (top_fp, _) in &top_files {
        let dependents = graph.dependents(top_fp);
        if let Some(node) = graph.deps(top_fp) {
            for dep in &node.depends_on {
                for (idx, chunk) in chunks.iter().enumerate() {
                    if chunk.file_path == *dep && !scores.contains_key(&idx) {
                        scores.insert(idx, boost * 0.5);
                    }
                }
            }
        }
        for dep_fp in dependents {
            for (idx, chunk) in chunks.iter().enumerate() {
                if chunk.file_path == dep_fp && !scores.contains_key(&idx) {
                    scores.insert(idx, boost * 0.3);
                }
            }
        }
    }
}

pub fn search_bm25(
    query: &str,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return Vec::new();
    }
    let mask = selector_to_mask(selector, chunks.len());
    let scores = bm25_index.get_scores(&tokens, mask.as_deref());

    let mut indexed: Vec<(usize, f64)> = scores
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > 0.0)
        .map(|(i, &s)| (i, s as f64))
        .collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(top_k);

    let results: Vec<SearchResult> = indexed
        .into_iter()
        .map(|(idx, score)| {
            let match_lines = find_match_lines(&chunks[idx], query);
            SearchResult {
                chunk: chunks[idx].clone(),
                score,
                match_lines,
            }
        })
        .collect();

    filter_low_scores(results)
}

#[allow(clippy::too_many_arguments)]
pub fn search_hybrid(
    query: &str,
    encoder: &StaticEncoder,
    semantic_index: &SemanticIndex,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    alpha: Option<f64>,
    selector: Option<&[usize]>,
    graph: Option<&DependencyGraph>,
) -> Vec<SearchResult> {
    let alpha_weight = resolve_alpha(query, alpha);
    let candidate_count = top_k * 5;

    let query_embedding = match encoder.encode_single(query) {
        Ok(e) => e,
        Err(_) => {
            return search_bm25(query, bm25_index, chunks, top_k, selector);
        }
    };
    let semantic_results = semantic_index.query(&query_embedding, candidate_count, selector);
    let semantic_scores: HashMap<usize, f64> = semantic_results
        .iter()
        .map(|&(idx, dist)| (idx, (1.0 - dist) as f64))
        .collect();

    let tokens = tokenize(query);
    let bm25_scores: HashMap<usize, f64> = if !tokens.is_empty() {
        let mask = selector_to_mask(selector, chunks.len());
        let raw_scores = bm25_index.get_scores(&tokens, mask.as_deref());
        let mut indexed: Vec<(usize, f64)> = raw_scores
            .iter()
            .enumerate()
            .filter(|(_, &s)| s > 0.0)
            .map(|(i, &s)| (i, s as f64))
            .collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        indexed.truncate(candidate_count);
        indexed.into_iter().collect()
    } else {
        HashMap::new()
    };

    let norm_semantic = rrf_scores(&semantic_scores);
    let norm_bm25 = rrf_scores(&bm25_scores);

    let all_indices: std::collections::HashSet<usize> = norm_semantic
        .keys()
        .chain(norm_bm25.keys())
        .cloned()
        .collect();
    let mut combined: HashMap<usize, f64> = HashMap::new();
    for idx in all_indices {
        let sem = norm_semantic.get(&idx).copied().unwrap_or(0.0);
        let bm = norm_bm25.get(&idx).copied().unwrap_or(0.0);
        combined.insert(idx, alpha_weight * sem + (1.0 - alpha_weight) * bm);
    }

    boost_multi_chunk_files(&mut combined, chunks);
    apply_query_boost(&mut combined, query, chunks);
    boost_sibling_chunks(&mut combined, chunks, query);

    if let Some(g) = graph {
        boost_from_graph(&mut combined, chunks, g);
    }

    let ranked = rerank_topk(&combined, chunks, top_k, alpha_weight < 1.0);

    let results: Vec<SearchResult> = ranked
        .into_iter()
        .map(|(idx, score)| {
            let match_lines = find_match_lines(&chunks[idx], query);
            SearchResult {
                chunk: chunks[idx].clone(),
                score,
                match_lines,
            }
        })
        .collect();

    filter_low_scores(results)
}
