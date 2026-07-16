pub mod create;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::bm25::Bm25Index;
use crate::encoder::{SemanticIndex, StaticEncoder};
use crate::graph::DependencyGraph;
use crate::search::search_hybrid;
use crate::stats::save_search_stats;
use crate::types::{CallType, Chunk, IndexStats, SearchResult};
use create::create_index_from_path;

use std::collections::HashSet;

pub struct SembleIndex {
    encoder: StaticEncoder,
    bm25_index: Bm25Index,
    semantic_index: SemanticIndex,
    chunks: Vec<Chunk>,
    #[allow(dead_code)]
    root: Option<PathBuf>,
    file_sizes: HashMap<String, usize>,
    file_mapping: HashMap<String, Vec<usize>>,
    language_mapping: HashMap<String, Vec<usize>>,
    graph: DependencyGraph,
}

impl SembleIndex {
    pub fn from_path(
        path: impl AsRef<Path>,
        encoder: Option<StaticEncoder>,
        extensions: Option<&HashSet<String>>,
        ignore: Option<&HashSet<String>>,
        include_text_files: bool,
    ) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            bail!("Path does not exist: {}", path.display());
        }
        if !path.is_dir() {
            bail!("Path is not a directory: {}", path.display());
        }
        let path = path.canonicalize().context("Failed to resolve path")?;
        let encoder = match encoder {
            Some(e) => e,
            None => StaticEncoder::load(None).context("Failed to load embedding model")?,
        };

        let (bm25_index, semantic_index, chunks, graph) = create_index_from_path(
            &path,
            &encoder,
            extensions,
            ignore,
            include_text_files,
            &path,
        )?;

        let file_sizes = compute_file_sizes(&path, &chunks);
        let (file_mapping, language_mapping) = build_mappings(&chunks);

        Ok(Self {
            encoder,
            bm25_index,
            semantic_index,
            chunks,
            root: Some(path),
            file_sizes,
            file_mapping,
            language_mapping,
            graph,
        })
    }

    pub fn from_git(
        url: &str,
        ref_: Option<&str>,
        encoder: Option<StaticEncoder>,
        extensions: Option<&HashSet<String>>,
        ignore: Option<&HashSet<String>>,
        include_text_files: bool,
    ) -> Result<Self> {
        let tmp_dir = std::env::temp_dir().join(format!("semble-clone-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir)?;

        let mut cmd = Command::new("git");
        cmd.args(["clone", "--depth", "1"]);
        if let Some(r) = ref_ {
            cmd.args(["--branch", r]);
        }
        cmd.args(["--", url, &tmp_dir.to_string_lossy()]);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());

        let output = cmd.output().map_err(|e| {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            anyhow::anyhow!("git is not installed or not on PATH: {e}")
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = std::fs::remove_dir_all(&tmp_dir);
            bail!("git clone failed for {url:?}:\n{}", stderr.trim());
        }

        let encoder = match encoder {
            Some(e) => e,
            None => StaticEncoder::load(None).context("Failed to load embedding model")?,
        };

        let resolved = tmp_dir.canonicalize().unwrap_or_else(|_| tmp_dir.clone());
        let result = create_index_from_path(
            &resolved,
            &encoder,
            extensions,
            ignore,
            include_text_files,
            &resolved,
        );

        let (bm25_index, semantic_index, chunks, graph) = match result {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp_dir);
                return Err(e);
            }
        };

        let file_sizes = compute_file_sizes(&resolved, &chunks);
        let (file_mapping, language_mapping) = build_mappings(&chunks);
        let _ = std::fs::remove_dir_all(&tmp_dir);

        Ok(Self {
            encoder,
            bm25_index,
            semantic_index,
            chunks,
            root: None,
            file_sizes,
            file_mapping,
            language_mapping,
            graph,
        })
    }

    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        alpha: Option<f64>,
        filter_languages: Option<&[String]>,
        filter_paths: Option<&[String]>,
    ) -> Vec<SearchResult> {
        if self.chunks.is_empty() || query.trim().is_empty() {
            return Vec::new();
        }

        let selector = self.get_selector(filter_languages, filter_paths);
        let selector_ref = selector.as_deref();

        let results = search_hybrid(
            query,
            &self.encoder,
            &self.semantic_index,
            &self.bm25_index,
            &self.chunks,
            top_k,
            alpha,
            selector_ref,
            Some(&self.graph),
        );

        save_search_stats(&results, CallType::Search, &self.file_sizes);
        results
    }

    pub fn find_related(&self, source: &Chunk, top_k: usize) -> Vec<SearchResult> {
        let selector = source
            .language
            .as_ref()
            .and_then(|lang| self.language_mapping.get(lang))
            .map(|indices| indices.as_slice());

        let query_embedding = match self.encoder.encode_single(&source.content) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let results = self
            .semantic_index
            .query(&query_embedding, top_k + 1, selector);
        let results: Vec<SearchResult> = results
            .into_iter()
            .filter(|&(idx, _)| self.chunks[idx] != *source)
            .take(top_k)
            .map(|(idx, dist)| SearchResult {
                chunk: self.chunks[idx].clone(),
                score: (1.0 - dist) as f64,
                match_lines: vec![],
            })
            .collect();

        save_search_stats(&results, CallType::FindRelated, &self.file_sizes);
        results
    }

    pub fn stats(&self) -> IndexStats {
        let mut language_counts: HashMap<String, usize> = HashMap::new();
        for chunk in &self.chunks {
            if let Some(lang) = &chunk.language {
                *language_counts.entry(lang.clone()).or_default() += 1;
            }
        }
        IndexStats {
            indexed_files: self.file_mapping.len(),
            total_chunks: self.chunks.len(),
            languages: language_counts,
        }
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }
    pub fn graph(&self) -> &DependencyGraph {
        &self.graph
    }

    fn get_selector(
        &self,
        filter_languages: Option<&[String]>,
        filter_paths: Option<&[String]>,
    ) -> Option<Vec<usize>> {
        let mut indices = Vec::new();
        if let Some(langs) = filter_languages {
            for lang in langs {
                if let Some(ids) = self.language_mapping.get(lang) {
                    indices.extend(ids);
                }
            }
        }
        if let Some(paths) = filter_paths {
            for path in paths {
                if let Some(ids) = self.file_mapping.get(path) {
                    indices.extend(ids);
                }
            }
        }
        if indices.is_empty() {
            None
        } else {
            indices.sort();
            indices.dedup();
            Some(indices)
        }
    }
}

fn compute_file_sizes(root: &Path, chunks: &[Chunk]) -> HashMap<String, usize> {
    let mut sizes: HashMap<String, usize> = HashMap::new();
    for chunk in chunks {
        if sizes.contains_key(&chunk.file_path) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(root.join(&chunk.file_path)) {
            sizes.insert(chunk.file_path.clone(), content.len());
        }
    }
    sizes
}

fn build_mappings(chunks: &[Chunk]) -> (HashMap<String, Vec<usize>>, HashMap<String, Vec<usize>>) {
    let mut file_mapping: HashMap<String, Vec<usize>> = HashMap::new();
    let mut language_mapping: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, chunk) in chunks.iter().enumerate() {
        file_mapping
            .entry(chunk.file_path.clone())
            .or_default()
            .push(i);
        if let Some(lang) = &chunk.language {
            language_mapping.entry(lang.clone()).or_default().push(i);
        }
    }
    (file_mapping, language_mapping)
}
