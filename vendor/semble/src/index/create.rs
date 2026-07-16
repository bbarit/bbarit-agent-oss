use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::bm25::Bm25Index;
use crate::chunking::chunk_source;
use crate::encoder::{SemanticIndex, StaticEncoder};
use crate::file_walker::{filter_extensions, language_for_path, walk_files};
use crate::graph::DependencyGraph;
use crate::tokens::tokenize;
use crate::types::Chunk;

const MAX_FILE_BYTES: u64 = 1_000_000;

fn enrich_for_bm25(chunk: &Chunk) -> String {
    let path = Path::new(&chunk.file_path);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let dir_parts: Vec<&str> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| {
                    let s = c.as_os_str().to_str()?;
                    if s == "." || s == "/" {
                        None
                    } else {
                        Some(s)
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dir_text: String = dir_parts
        .iter()
        .rev()
        .take(3)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} {stem} {stem} {dir_text}", chunk.content)
}

pub fn create_index_from_path(
    path: &Path,
    encoder: &StaticEncoder,
    extensions: Option<&HashSet<String>>,
    ignore: Option<&HashSet<String>>,
    include_text_files: bool,
    display_root: &Path,
) -> Result<(Bm25Index, SemanticIndex, Vec<Chunk>, DependencyGraph)> {
    let exts = filter_extensions(extensions, include_text_files);
    let files = walk_files(path, &exts, ignore);

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut graph = DependencyGraph::new();

    for file_path in &files {
        let metadata = match file_path.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let language = language_for_path(file_path);
        let chunk_path = file_path
            .strip_prefix(display_root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();
        chunks.extend(chunk_source(&source, &chunk_path, language));

        if let Some(lang) = language {
            graph.add_file(&chunk_path, &source, lang);
        }
    }

    if chunks.is_empty() {
        bail!("No supported files found under {}", path.display());
    }

    graph.resolve_dependencies();

    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let embeddings = encoder
        .encode_batch(&texts)
        .context("Failed to encode chunks")?;
    let semantic_index = SemanticIndex::new(embeddings);

    let bm25_docs: Vec<Vec<String>> = chunks
        .iter()
        .map(|chunk| tokenize(&enrich_for_bm25(chunk)))
        .collect();
    let bm25_index = Bm25Index::new(&bm25_docs);

    Ok((bm25_index, semantic_index, chunks, graph))
}
