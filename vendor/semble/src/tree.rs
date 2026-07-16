use std::collections::BTreeMap;

use crate::graph::{DependencyGraph, Symbol};
use crate::types::Chunk;

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    is_file: bool,
    file_path: Option<String>,
}

pub struct TreeOptions<'a> {
    pub dirs_only: bool,
    pub max_depth: Option<usize>,
    pub symbols: bool,
    pub langs: Option<&'a [String]>,
}

pub fn render(chunks: &[Chunk], graph: &DependencyGraph, opts: &TreeOptions) -> String {
    // Collect unique file paths from chunks (already gitignore-filtered by index).
    let mut paths: Vec<&str> = chunks
        .iter()
        .filter(|c| match opts.langs {
            Some(filters) => c
                .language
                .as_deref()
                .map(|l| filters.iter().any(|f| f == l))
                .unwrap_or(false),
            None => true,
        })
        .map(|c| c.file_path.as_str())
        .collect();
    paths.sort();
    paths.dedup();

    let mut root = Node::default();
    for p in &paths {
        insert_path(&mut root, p);
    }

    let mut out = String::new();
    let mut prefix = String::new();
    render_node(&root, &mut out, &mut prefix, true, 0, graph, opts);
    out
}

fn insert_path(root: &mut Node, path: &str) {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur = root;
    for (i, part) in parts.iter().enumerate() {
        cur = cur.children.entry((*part).to_string()).or_default();
        if i + 1 == parts.len() {
            cur.is_file = true;
            cur.file_path = Some(path.to_string());
        }
    }
}

fn render_node(
    node: &Node,
    out: &mut String,
    prefix: &mut String,
    is_root: bool,
    depth: usize,
    graph: &DependencyGraph,
    opts: &TreeOptions,
) {
    if let Some(max) = opts.max_depth {
        if depth > max {
            return;
        }
    }
    let entries: Vec<(&String, &Node)> = node
        .children
        .iter()
        .filter(|(_, child)| !opts.dirs_only || !child.is_file || !child.children.is_empty())
        .collect();

    let last_idx = entries.len().saturating_sub(1);
    for (i, (name, child)) in entries.iter().enumerate() {
        let is_last = i == last_idx;
        let connector = if is_root {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };
        let display = if child.is_file && !child.children.is_empty() {
            format!("{name}/")
        } else if !child.is_file {
            format!("{name}/")
        } else {
            (*name).clone()
        };

        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(&display);

        if opts.symbols && child.is_file {
            if let Some(path) = &child.file_path {
                if let Some(syms) = top_level_symbols(graph, path) {
                    out.push_str("  (");
                    out.push_str(&syms);
                    out.push(')');
                }
            }
        }
        out.push('\n');

        if !child.children.is_empty() {
            let push = if is_root {
                ""
            } else if is_last {
                "    "
            } else {
                "│   "
            };
            prefix.push_str(push);
            render_node(child, out, prefix, false, depth + 1, graph, opts);
            prefix.truncate(prefix.len() - push.len());
        }
    }
}

fn top_level_symbols(graph: &DependencyGraph, file_path: &str) -> Option<String> {
    let node = graph.files.get(file_path)?;
    let names: Vec<String> = node
        .symbols
        .iter()
        .filter(|s| is_top_level_kind(&s.kind))
        .take(6)
        .map(|s: &Symbol| s.name.clone())
        .collect();
    if names.is_empty() {
        None
    } else {
        let extra = node
            .symbols
            .iter()
            .filter(|s| is_top_level_kind(&s.kind))
            .count()
            .saturating_sub(names.len());
        let mut s = names.join(", ");
        if extra > 0 {
            s.push_str(&format!(", +{extra}"));
        }
        Some(s)
    }
}

fn is_top_level_kind(kind: &str) -> bool {
    matches!(
        kind,
        "fn" | "function"
            | "struct"
            | "class"
            | "enum"
            | "trait"
            | "interface"
            | "type"
            | "impl"
            | "module"
    )
}
