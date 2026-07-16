use std::io::Read;
use std::process;

use clap::{Parser, Subcommand};

use semble::digest::{self, Format};
use semble::encoder::StaticEncoder;
use semble::filter::smart_strip;
use semble::index::SembleIndex;
use semble::outline::extract_signature_near;
use semble::plan::{build_plan, print_plan};
use semble::stats::format_savings_report;
use semble::tree::{render as render_tree, TreeOptions};
use semble::types::SearchResult;
use semble::utils::{format_results, is_git_url, resolve_chunk};

#[derive(Parser)]
#[command(
    name = "semble_rs",
    version,
    about = "Fast and Accurate Code Search for Agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search a codebase with keyword/symbol query
    Search {
        /// Keyword, symbol, or function name to search for
        query: String,
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Number of results
        #[arg(short = 'k', long = "top-k", default_value = "10")]
        top_k: usize,
        /// Also index non-code text files (.md, .yaml, .json, etc.)
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Compact output: file paths, scores, and match lines only (minimal tokens)
        #[arg(long)]
        compact: bool,
        /// Strip comments from code chunks in JSON output to reduce tokens
        #[arg(long)]
        strip: bool,
        /// Outline output: one signature line per chunk (smallest token footprint)
        #[arg(long)]
        outline: bool,
        /// Group results by directory + cap match lines at 3 per chunk
        #[arg(long)]
        group: bool,
        /// Embedding model (HF repo id or local path).
        /// Overrides SEMBLE_MODEL_PATH; default: minishlab/potion-code-16M.
        #[arg(long)]
        model: Option<String>,
    },
    /// Find code similar to a specific location
    FindRelated {
        /// File path as shown in search results
        file_path: String,
        /// Line number (1-indexed)
        line: usize,
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Number of results
        #[arg(short = 'k', long = "top-k", default_value = "10")]
        top_k: usize,
        /// Also index non-code text files
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Embedding model (HF repo id or local path).
        #[arg(long)]
        model: Option<String>,
    },
    /// Show what a file depends on and what symbols it defines
    Deps {
        /// File path (relative to project root)
        file_path: String,
        /// Local path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Output as Graphviz DOT (pipe into `dot -Tpng > graph.png`)
        #[arg(long)]
        dot: bool,
        /// Output as ASCII dependency tree (transitive imports)
        #[arg(long)]
        tree: bool,
        /// Max tree depth (with --tree)
        #[arg(long)]
        max_depth: Option<usize>,
    },
    /// Show all files affected if a file changes (transitive)
    Impact {
        /// File path (relative to project root)
        file_path: String,
        /// Local path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Output as Graphviz DOT
        #[arg(long)]
        dot: bool,
        /// Output as ASCII reverse-dependency tree (who depends on this)
        #[arg(long)]
        tree: bool,
        /// Max tree depth (with --tree)
        #[arg(long)]
        max_depth: Option<usize>,
    },
    /// AST pattern match — wraps `ast-grep` for "find every `fn $name($$$)`"
    /// style structural queries that semantic search can't express.
    FindPattern {
        /// ast-grep pattern, e.g. `"fn $name($$$)"`
        pattern: String,
        /// Local path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Language hint passed to ast-grep (rust, python, javascript, ...)
        #[arg(long)]
        lang: Option<String>,
        /// Compact one-line-per-match output
        #[arg(long)]
        compact: bool,
    },
    /// Recommend a token-efficient exploration flow for a task
    Plan {
        /// Natural-language task or feature to investigate
        task: String,
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Number of candidate chunks to use
        #[arg(short = 'k', long = "top-k", default_value = "8")]
        top_k: usize,
        /// Also index non-code text files (.md, .yaml, .json, etc.)
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Embedding model (HF repo id or local path).
        #[arg(long)]
        model: Option<String>,
    },
    /// Show token savings and usage stats
    Savings {
        /// Show usage breakdown by call type
        #[arg(long)]
        verbose: bool,
    },
    /// Print the codebase file tree (gitignore-aware, no `ls -R` token explosion)
    Tree {
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Show directories only
        #[arg(short = 'd', long)]
        dirs_only: bool,
        /// Limit tree depth
        #[arg(long)]
        max_depth: Option<usize>,
        /// Append top-level symbols (fn, struct, class, enum, ...) per file
        #[arg(long)]
        symbols: bool,
        /// Filter languages (comma-separated, e.g. rust,python)
        #[arg(long, value_delimiter = ',')]
        lang: Option<Vec<String>>,
        /// Also index non-code text files
        #[arg(long)]
        include_text_files: bool,
    },
    /// Encode text to a Model2Vec embedding vector (JSON output)
    Encode {
        /// Text to encode. If omitted, reads sentences from --file or stdin (one per line).
        text: Option<String>,
        /// Read sentences from a file (one per line).
        #[arg(long)]
        file: Option<String>,
        /// Override SEMBLE_MODEL_PATH / default model (HF repo id or local path).
        #[arg(long)]
        model: Option<String>,
    },
    /// Compress build/test/install/CI output (cargo, pnpm, tsc, pytest, GitHub Actions)
    Digest {
        /// Input file. If omitted, reads from stdin.
        file: Option<String>,
        /// Force a specific format (auto-detects if omitted).
        /// Values: cargo, pnpm, tsc, pytest, ci.
        #[arg(long, default_value = "auto")]
        format: String,
        /// Print the detected format on stderr.
        #[arg(long)]
        show_format: bool,
    },
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Tree {
            path,
            dirs_only,
            max_depth,
            symbols,
            lang,
            include_text_files,
        } => {
            let index = build_index(&path, include_text_files, None);
            let opts = TreeOptions {
                dirs_only,
                max_depth,
                symbols,
                langs: lang.as_deref(),
            };
            let out = render_tree(index.chunks(), index.graph(), &opts);
            print!("{out}");
        }
        Commands::Encode { text, file, model } => {
            let encoder = StaticEncoder::load(model.as_deref()).unwrap_or_else(|e| {
                eprintln!("Failed to load model: {e}");
                process::exit(1);
            });
            let inputs: Vec<String> = if let Some(t) = text {
                vec![t]
            } else {
                let buf = if let Some(f) = file {
                    std::fs::read_to_string(&f).unwrap_or_else(|e| {
                        eprintln!("Error reading {f}: {e}");
                        process::exit(1);
                    })
                } else {
                    let mut s = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                        eprintln!("Error reading stdin: {e}");
                        process::exit(1);
                    }
                    s
                };
                let lines: Vec<String> = buf
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if lines.is_empty() {
                    eprintln!("No input text.");
                    process::exit(1);
                }
                lines
            };
            let arr = encoder.encode_batch(&inputs).unwrap_or_else(|e| {
                eprintln!("Encoding failed: {e}");
                process::exit(1);
            });
            let rows: Vec<Vec<f32>> = arr.outer_iter().map(|r| r.to_vec()).collect();
            let json = if rows.len() == 1 {
                serde_json::to_string(&rows[0])
            } else {
                serde_json::to_string(&rows)
            }
            .unwrap_or_else(|e| {
                eprintln!("Serialization failed: {e}");
                process::exit(1);
            });
            println!("{json}");
        }
        Commands::Digest {
            file,
            format,
            show_format,
        } => {
            let text = match file {
                Some(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
                    eprintln!("Error reading {path}: {e}");
                    process::exit(1);
                }),
                None => {
                    let mut buf = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                        eprintln!("Error reading stdin: {e}");
                        process::exit(1);
                    }
                    buf
                }
            };
            let fmt = if format == "auto" {
                digest::detect(&text)
            } else {
                Format::parse(&format).unwrap_or_else(|| {
                    eprintln!("Unknown --format value: {format}. Valid: cargo, pnpm, tsc, pytest, ci, auto.");
                    process::exit(1);
                })
            };
            if show_format {
                eprintln!("[digest] format={}", fmt.as_str());
            }
            let out = digest::digest(&text, fmt);
            println!("{out}");
        }
        Commands::FindPattern {
            pattern,
            path,
            lang,
            compact,
        } => {
            // Thin wrapper around `ast-grep` for structural pattern matching.
            // Falls back to a clear hint if ast-grep isn't installed.
            let mut cmd = std::process::Command::new("ast-grep");
            cmd.arg("--pattern").arg(&pattern).arg(&path);
            if let Some(l) = lang.as_deref() {
                cmd.arg("--lang").arg(l);
            }
            if compact {
                cmd.arg("--json=stream");
            }
            match cmd.spawn() {
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(_) => {
                    eprintln!(
                        "ast-grep is not installed. semble_rs find-pattern is a thin wrapper around it.\n\
                         Install with `brew install ast-grep` or `cargo install ast-grep` and re-run."
                    );
                    process::exit(1);
                }
            }
        }
        Commands::Savings { verbose } => {
            print!("{}", format_savings_report(verbose));
        }
        Commands::Deps {
            file_path,
            path,
            json,
            dot,
            tree,
            max_depth,
        } => {
            let index = build_index(&path, false, None);
            let graph = index.graph();

            if dot {
                println!("{}", graph.deps_dot(&file_path));
                return;
            }
            if tree {
                if graph.deps(&file_path).is_none() {
                    eprintln!("File not found in graph: {file_path}");
                    process::exit(1);
                }
                print!("{}", render_dep_tree(graph, &file_path, max_depth, false));
                return;
            }
            if json {
                match graph.deps(&file_path) {
                    Some(node) => {
                        println!(
                            "{}",
                            serde_json::to_string(node).unwrap_or_else(|_| "{}".to_string())
                        );
                    }
                    None => {
                        println!("{{}}");
                    }
                }
            } else {
                match graph.deps(&file_path) {
                    Some(node) => {
                        println!("File: {file_path}");
                        println!();
                        if !node.symbols.is_empty() {
                            println!("Symbols ({}):", node.symbols.len());
                            for sym in &node.symbols {
                                println!("  {} {} (line {})", sym.kind, sym.name, sym.line);
                            }
                            println!();
                        }
                        if !node.depends_on.is_empty() {
                            println!("Depends on ({}):", node.depends_on.len());
                            for dep in &node.depends_on {
                                println!("  {dep}");
                            }
                            println!();
                        }
                        let dependents = graph.dependents(&file_path);
                        if !dependents.is_empty() {
                            println!("Used by ({}):", dependents.len());
                            for dep in &dependents {
                                println!("  {dep}");
                            }
                        }
                        if node.symbols.is_empty()
                            && node.depends_on.is_empty()
                            && dependents.is_empty()
                        {
                            println!("No dependencies or symbols found.");
                        }
                    }
                    None => {
                        eprintln!("File not found in graph: {file_path}");
                        process::exit(1);
                    }
                }
            }
        }
        Commands::Impact {
            file_path,
            path,
            json,
            dot,
            tree,
            max_depth,
        } => {
            let index = build_index(&path, false, None);
            let graph = index.graph();

            if dot {
                println!("{}", graph.impact_dot(&file_path));
                return;
            }
            if tree {
                if graph.deps(&file_path).is_none() && graph.dependents(&file_path).is_empty() {
                    eprintln!("File not found in graph: {file_path}");
                    process::exit(1);
                }
                print!("{}", render_dep_tree(graph, &file_path, max_depth, true));
                return;
            }
            let affected = graph.impact(&file_path);

            if json {
                println!(
                    "{}",
                    serde_json::to_string(&affected).unwrap_or_else(|_| "[]".to_string())
                );
            } else if affected.is_empty() {
                println!("No files affected by changes to {file_path}.");
            } else {
                println!("Impact of {file_path} ({} files affected):", affected.len());
                println!();
                for f in &affected {
                    println!("  {f}");
                }
            }
        }
        Commands::Plan {
            task,
            path,
            top_k,
            include_text_files,
            json,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());
            let results = index.search(task.as_str(), top_k, None, None, None);
            let report = build_plan(&task, &path, top_k, &results);

            if json {
                println!(
                    "{}",
                    serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                print_plan(&report);
            }
        }
        Commands::Search {
            query,
            path,
            top_k,
            include_text_files,
            json,
            compact,
            strip,
            outline,
            group,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());

            let results = index.search(query.as_str(), top_k, None, None, None);
            if outline {
                print_outline(&results);
            } else if group {
                print_grouped(&results);
            } else if compact {
                print_compact(&results);
            } else if json && strip {
                print_json_stripped(&results);
            } else if json {
                print_json(&results);
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                println!(
                    "{}",
                    format_results(&format!("Search results for: {query:?}"), &results)
                );
            }
        }
        Commands::FindRelated {
            file_path,
            line,
            path,
            top_k,
            include_text_files,
            json,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());

            let chunk = match resolve_chunk(index.chunks(), &file_path, line) {
                Some(c) => c.clone(),
                None => {
                    eprintln!("No chunk found at {file_path}:{line}.");
                    process::exit(1);
                }
            };

            let results = index.find_related(&chunk, top_k);
            if json {
                print_json(&results);
            } else if results.is_empty() {
                println!("No related chunks found for {file_path}:{line}.");
            } else {
                println!(
                    "{}",
                    format_results(&format!("Chunks related to {file_path}:{line}"), &results)
                );
            }
        }
    }
}

fn print_compact(results: &[SearchResult]) {
    for r in results {
        println!(
            "{:.4}\t{}:{}-{}",
            r.score, r.chunk.file_path, r.chunk.start_line, r.chunk.end_line
        );
        for ml in &r.match_lines {
            println!("  L{}:\t{}", ml.line, truncate_line(&ml.content, 120));
        }
    }
}

fn print_outline(results: &[SearchResult]) {
    for r in results {
        let match_nums: Vec<usize> = r.match_lines.iter().map(|m| m.line).collect();
        let sig = extract_signature_near(&r.chunk.content, r.chunk.start_line, &match_nums)
            .unwrap_or_else(|| format!("(lines {}-{})", r.chunk.start_line, r.chunk.end_line));
        let match_suffix = if r.match_lines.is_empty() {
            String::new()
        } else {
            format!(" [{}m]", r.match_lines.len())
        };
        println!(
            "{:.4} {}:{}-{}{}\n  {}",
            r.score, r.chunk.file_path, r.chunk.start_line, r.chunk.end_line, match_suffix, sig
        );
    }
}

fn print_grouped(results: &[SearchResult]) {
    use std::collections::BTreeMap;
    let mut by_dir: BTreeMap<String, (f64, Vec<&SearchResult>)> = BTreeMap::new();
    for r in results {
        let dir = std::path::Path::new(&r.chunk.file_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let entry = by_dir.entry(dir).or_insert((f64::NEG_INFINITY, Vec::new()));
        if r.score > entry.0 {
            entry.0 = r.score;
        }
        entry.1.push(r);
    }
    let mut dirs: Vec<(&String, &(f64, Vec<&SearchResult>))> = by_dir.iter().collect();
    dirs.sort_by(|a, b| {
        b.1 .0
            .partial_cmp(&a.1 .0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    const MAX_MATCH_LINES: usize = 3;
    for (dir, (_, group)) in dirs {
        let has_dir = !dir.is_empty();
        if has_dir {
            println!("{dir}/");
        }
        for r in group {
            let fname = std::path::Path::new(&r.chunk.file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(r.chunk.file_path.as_str());
            let indent = if has_dir { "  " } else { "" };
            println!(
                "{indent}{:.4} {fname}:{}-{}",
                r.score, r.chunk.start_line, r.chunk.end_line
            );
            let total = r.match_lines.len();
            for ml in r.match_lines.iter().take(MAX_MATCH_LINES) {
                println!(
                    "{indent}  L{}: {}",
                    ml.line,
                    truncate_line(&ml.content, 100)
                );
            }
            if total > MAX_MATCH_LINES {
                println!("{indent}  ... (+{})", total - MAX_MATCH_LINES);
            }
        }
    }
}

fn render_dep_tree(
    graph: &semble::DependencyGraph,
    root: &str,
    max_depth: Option<usize>,
    reverse: bool,
) -> String {
    let mut out = String::new();
    out.push_str(root);
    out.push('\n');
    let mut visited = std::collections::HashSet::new();
    visited.insert(root.to_string());
    let children = next_files(graph, root, reverse);
    let mut prefix = String::new();
    walk_dep_tree(
        graph,
        &children,
        &mut visited,
        &mut prefix,
        &mut out,
        1,
        max_depth,
        reverse,
    );
    out
}

fn next_files(graph: &semble::DependencyGraph, file: &str, reverse: bool) -> Vec<String> {
    if reverse {
        graph
            .dependents(file)
            .into_iter()
            .map(String::from)
            .collect()
    } else {
        graph
            .deps(file)
            .map(|n| n.depends_on.clone())
            .unwrap_or_default()
    }
}

fn walk_dep_tree(
    graph: &semble::DependencyGraph,
    items: &[String],
    visited: &mut std::collections::HashSet<String>,
    prefix: &mut String,
    out: &mut String,
    depth: usize,
    max_depth: Option<usize>,
    reverse: bool,
) {
    let last_idx = items.len().saturating_sub(1);
    for (i, item) in items.iter().enumerate() {
        let is_last = i == last_idx;
        let connector = if is_last { "└── " } else { "├── " };
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(item);

        let cycle = visited.contains(item);
        if cycle {
            out.push_str("  (cycle)\n");
            continue;
        }
        let depth_exceeded = max_depth.is_some_and(|m| depth >= m);
        let children = if depth_exceeded {
            vec![]
        } else {
            next_files(graph, item, reverse)
        };
        if depth_exceeded && !next_files(graph, item, reverse).is_empty() {
            out.push_str("  …\n");
            continue;
        }
        out.push('\n');

        if !children.is_empty() {
            visited.insert(item.clone());
            let push = if is_last { "    " } else { "│   " };
            prefix.push_str(push);
            walk_dep_tree(
                graph,
                &children,
                visited,
                prefix,
                out,
                depth + 1,
                max_depth,
                reverse,
            );
            prefix.truncate(prefix.len() - push.len());
            visited.remove(item);
        }
    }
}

fn truncate_line(line: &str, max_len: usize) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }
    let s: String = trimmed.chars().take(max_len - 3).collect();
    format!("{s}...")
}

fn print_json_stripped(results: &[SearchResult]) {
    let stripped: Vec<SearchResult> = results
        .iter()
        .map(|r| {
            let lang = r.chunk.language.as_deref();
            SearchResult {
                chunk: semble::types::Chunk::new(
                    smart_strip(&r.chunk.content, lang),
                    r.chunk.file_path.clone(),
                    r.chunk.start_line,
                    r.chunk.end_line,
                    r.chunk.language.clone(),
                ),
                score: r.score,
                match_lines: r.match_lines.clone(),
            }
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string(&stripped).unwrap_or_else(|_| "[]".to_string())
    );
}

fn print_json(results: &[SearchResult]) {
    println!(
        "{}",
        serde_json::to_string(results).unwrap_or_else(|_| "[]".to_string())
    );
}

fn build_index(path: &str, include_text_files: bool, model: Option<&str>) -> SembleIndex {
    let encoder = model.map(|m| {
        StaticEncoder::load(Some(m)).unwrap_or_else(|e| {
            eprintln!("Failed to load model {m:?}: {e}");
            process::exit(1);
        })
    });
    let result = if is_git_url(path) {
        SembleIndex::from_git(path, None, encoder, None, None, include_text_files)
    } else {
        SembleIndex::from_path(path, encoder, None, None, include_text_files)
    };

    match result {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("Error: {e:?}");
            process::exit(1);
        }
    }
}
