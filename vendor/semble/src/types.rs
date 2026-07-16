use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Chunk {
    pub content: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub language: Option<String>,
}

impl Chunk {
    pub fn new(
        content: String,
        file_path: String,
        start_line: usize,
        end_line: usize,
        language: Option<String>,
    ) -> Self {
        Self {
            content,
            file_path,
            start_line,
            end_line,
            language,
        }
    }

    pub fn location(&self) -> String {
        format!("{}:{}-{}", self.file_path, self.start_line, self.end_line)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MatchLine {
    pub line: usize,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub chunk: Chunk,
    pub score: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub match_lines: Vec<MatchLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallType {
    Search,
    FindRelated,
}

impl fmt::Display for CallType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CallType::Search => write!(f, "search"),
            CallType::FindRelated => write!(f, "find_related"),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IndexStats {
    pub indexed_files: usize,
    pub total_chunks: usize,
    pub languages: HashMap<String, usize>,
}
