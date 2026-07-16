use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Code,
    Document,
}

#[derive(Debug, Clone)]
pub struct FileType {
    pub language: &'static str,
    pub category: FileCategory,
}

pub fn file_types() -> Vec<(&'static str, FileType)> {
    vec![
        (
            ".py",
            FileType {
                language: "python",
                category: FileCategory::Code,
            },
        ),
        (
            ".js",
            FileType {
                language: "javascript",
                category: FileCategory::Code,
            },
        ),
        (
            ".jsx",
            FileType {
                language: "javascript",
                category: FileCategory::Code,
            },
        ),
        (
            ".ts",
            FileType {
                language: "typescript",
                category: FileCategory::Code,
            },
        ),
        (
            ".tsx",
            FileType {
                language: "typescript",
                category: FileCategory::Code,
            },
        ),
        (
            ".go",
            FileType {
                language: "go",
                category: FileCategory::Code,
            },
        ),
        (
            ".rs",
            FileType {
                language: "rust",
                category: FileCategory::Code,
            },
        ),
        (
            ".java",
            FileType {
                language: "java",
                category: FileCategory::Code,
            },
        ),
        (
            ".kt",
            FileType {
                language: "kotlin",
                category: FileCategory::Code,
            },
        ),
        (
            ".kts",
            FileType {
                language: "kotlin",
                category: FileCategory::Code,
            },
        ),
        (
            ".rb",
            FileType {
                language: "ruby",
                category: FileCategory::Code,
            },
        ),
        (
            ".php",
            FileType {
                language: "php",
                category: FileCategory::Code,
            },
        ),
        (
            ".c",
            FileType {
                language: "c",
                category: FileCategory::Code,
            },
        ),
        (
            ".h",
            FileType {
                language: "c",
                category: FileCategory::Code,
            },
        ),
        (
            ".cpp",
            FileType {
                language: "cpp",
                category: FileCategory::Code,
            },
        ),
        (
            ".hpp",
            FileType {
                language: "cpp",
                category: FileCategory::Code,
            },
        ),
        (
            ".cs",
            FileType {
                language: "csharp",
                category: FileCategory::Code,
            },
        ),
        (
            ".swift",
            FileType {
                language: "swift",
                category: FileCategory::Code,
            },
        ),
        (
            ".scala",
            FileType {
                language: "scala",
                category: FileCategory::Code,
            },
        ),
        (
            ".sbt",
            FileType {
                language: "scala",
                category: FileCategory::Code,
            },
        ),
        (
            ".ex",
            FileType {
                language: "elixir",
                category: FileCategory::Code,
            },
        ),
        (
            ".exs",
            FileType {
                language: "elixir",
                category: FileCategory::Code,
            },
        ),
        (
            ".dart",
            FileType {
                language: "dart",
                category: FileCategory::Code,
            },
        ),
        (
            ".lua",
            FileType {
                language: "lua",
                category: FileCategory::Code,
            },
        ),
        (
            ".sql",
            FileType {
                language: "sql",
                category: FileCategory::Code,
            },
        ),
        (
            ".sh",
            FileType {
                language: "bash",
                category: FileCategory::Code,
            },
        ),
        (
            ".bash",
            FileType {
                language: "bash",
                category: FileCategory::Code,
            },
        ),
        (
            ".zig",
            FileType {
                language: "zig",
                category: FileCategory::Code,
            },
        ),
        (
            ".hs",
            FileType {
                language: "haskell",
                category: FileCategory::Code,
            },
        ),
        (
            ".md",
            FileType {
                language: "markdown",
                category: FileCategory::Document,
            },
        ),
        (
            ".yaml",
            FileType {
                language: "yaml",
                category: FileCategory::Document,
            },
        ),
        (
            ".yml",
            FileType {
                language: "yaml",
                category: FileCategory::Document,
            },
        ),
        (
            ".toml",
            FileType {
                language: "toml",
                category: FileCategory::Document,
            },
        ),
        (
            ".json",
            FileType {
                language: "json",
                category: FileCategory::Document,
            },
        ),
        (
            ".html",
            FileType {
                language: "html",
                category: FileCategory::Code,
            },
        ),
        (
            ".htm",
            FileType {
                language: "html",
                category: FileCategory::Code,
            },
        ),
        (
            ".vue",
            FileType {
                language: "vue",
                category: FileCategory::Code,
            },
        ),
        (
            ".svelte",
            FileType {
                language: "svelte",
                category: FileCategory::Code,
            },
        ),
        (
            ".css",
            FileType {
                language: "css",
                category: FileCategory::Code,
            },
        ),
        (
            ".scss",
            FileType {
                language: "scss",
                category: FileCategory::Code,
            },
        ),
        (
            ".sass",
            FileType {
                language: "scss",
                category: FileCategory::Code,
            },
        ),
        (
            ".less",
            FileType {
                language: "css",
                category: FileCategory::Code,
            },
        ),
    ]
}

pub fn language_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    let dot_ext = format!(".{}", ext.to_lowercase());
    file_types()
        .iter()
        .find(|(e, _)| *e == dot_ext)
        .map(|(_, ft)| ft.language)
}

pub fn default_ignored_dirs() -> HashSet<String> {
    [
        ".git",
        ".hg",
        ".svn",
        "__pycache__",
        "node_modules",
        ".venv",
        "venv",
        ".tox",
        ".mypy_cache",
        ".pytest_cache",
        ".ruff_cache",
        ".cache",
        ".semble",
        ".next",
        "dist",
        "build",
        ".eggs",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn filter_extensions(
    extensions: Option<&HashSet<String>>,
    include_text_files: bool,
) -> HashSet<String> {
    if let Some(ext) = extensions {
        return ext.clone();
    }
    let mut categories = HashSet::new();
    categories.insert(FileCategory::Code);
    if include_text_files {
        categories.insert(FileCategory::Document);
    }
    file_types()
        .iter()
        .filter(|(_, ft)| categories.contains(&ft.category))
        .map(|(ext, _)| ext.to_string())
        .collect()
}

pub fn walk_files(
    root: &Path,
    extensions: &HashSet<String>,
    ignore_dirs: Option<&HashSet<String>>,
) -> Vec<PathBuf> {
    let default_ignored = default_ignored_dirs();
    let all_ignored: HashSet<&str> = default_ignored
        .iter()
        .map(|s| s.as_str())
        .chain(
            ignore_dirs
                .into_iter()
                .flat_map(|s| s.iter().map(|s| s.as_str())),
        )
        .collect();

    let mut ob = OverrideBuilder::new(root);
    for dir in &all_ignored {
        let _ = ob.add(&format!("!**/{dir}"));
    }
    let overrides = ob
        .build()
        .unwrap_or_else(|_| OverrideBuilder::new(root).build().unwrap());

    let walker = WalkBuilder::new(root)
        .overrides(overrides)
        .hidden(false)
        .parents(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    let mut files = Vec::new();
    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let dot_ext = format!(".{}", ext.to_lowercase());
            if extensions.contains(&dot_ext) {
                if is_likely_minified(path) {
                    continue;
                }
                files.push(path.to_path_buf());
            }
        }
    }

    files
}

/// Heuristic to skip minified / bundled assets that pollute search results.
/// Filename-pattern only: `.min.js`, `.min.css`, `.bundle.js`, etc.
pub fn is_likely_minified(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        for pat in MINIFIED_FILENAME_PATTERNS {
            if lower.contains(pat) {
                return true;
            }
        }
    }
    false
}

const MINIFIED_FILENAME_PATTERNS: &[&str] = &[
    ".min.js",
    ".min.mjs",
    ".min.css",
    ".bundle.js",
    ".bundle.css",
    "-min.js",
    "-min.css",
    ".prod.js",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_dot_min_suffix() {
        assert!(is_likely_minified(&PathBuf::from("vendor.min.js")));
        assert!(is_likely_minified(&PathBuf::from("a/b/lib.min.css")));
    }

    #[test]
    fn detects_bundle_pattern() {
        assert!(is_likely_minified(&PathBuf::from("app.bundle.js")));
        assert!(is_likely_minified(&PathBuf::from("dist/styles.bundle.css")));
    }

    #[test]
    fn detects_dash_min() {
        assert!(is_likely_minified(&PathBuf::from("lib-min.js")));
    }

    #[test]
    fn skips_normal_source_files() {
        assert!(!is_likely_minified(&PathBuf::from("normal.js")));
        assert!(!is_likely_minified(&PathBuf::from("src/main.rs")));
        assert!(!is_likely_minified(&PathBuf::from("auth.ts")));
    }
}
