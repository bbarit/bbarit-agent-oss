use once_cell::sync::Lazy;
use regex::Regex;

static MULTIPLE_BLANK_LINES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());

static FUNC_SIGNATURE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:pub\s+)?(?:async\s+)?(?:fn|def|function|func|class|struct|enum|trait|interface|type|export)\s+\w+"
    ).unwrap()
});

static IMPORT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:use |import |from |require\(|#include |const \{|module )").unwrap()
});

pub fn strip_comments(content: &str, lang: Option<&str>) -> String {
    let patterns = CommentSyntax::from_lang(lang);
    let mut result = String::with_capacity(content.len());
    let mut in_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if let (Some(start), Some(end)) = (patterns.block_start, patterns.block_end) {
            if !in_block && trimmed.contains(start) {
                in_block = true;
            }
            if in_block {
                if trimmed.contains(end) {
                    in_block = false;
                }
                continue;
            }
        }

        if let Some(prefix) = patterns.line_prefix {
            if trimmed.starts_with(prefix)
                && !trimmed.starts_with(patterns.doc_prefix.unwrap_or("///"))
            {
                continue;
            }
        }

        if trimmed.is_empty() {
            result.push('\n');
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    let result = MULTIPLE_BLANK_LINES.replace_all(&result, "\n\n");
    result.trim().to_string()
}

pub fn smart_strip(content: &str, lang: Option<&str>) -> String {
    let stripped = strip_comments(content, lang);
    smart_truncate(&stripped)
}

fn smart_truncate(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::with_capacity(lines.len());
    let mut brace_depth: i32 = 0;
    let mut in_body = false;
    let mut skipped = 0usize;

    for line in &lines {
        let trimmed = line.trim();

        if IMPORT_PATTERN.is_match(trimmed) {
            flush_skipped(&mut result, &mut skipped);
            result.push((*line).to_string());
            continue;
        }

        if FUNC_SIGNATURE.is_match(trimmed) {
            flush_skipped(&mut result, &mut skipped);
            result.push((*line).to_string());
            in_body = true;
            brace_depth = 0;
            continue;
        }

        if in_body {
            let open = trimmed.matches('{').count() as i32;
            let close = trimmed.matches('}').count() as i32;
            brace_depth += open - close;

            if trimmed == "{" || trimmed == "}" || trimmed.ends_with('{') {
                flush_skipped(&mut result, &mut skipped);
                result.push((*line).to_string());
            } else {
                skipped += 1;
            }

            if brace_depth <= 0 {
                in_body = false;
                flush_skipped(&mut result, &mut skipped);
            }
            continue;
        }

        flush_skipped(&mut result, &mut skipped);
        result.push((*line).to_string());
    }

    flush_skipped(&mut result, &mut skipped);
    result.join("\n")
}

fn flush_skipped(result: &mut Vec<String>, skipped: &mut usize) {
    if *skipped > 0 {
        result.push(format!("    [... {} lines]", skipped));
        *skipped = 0;
    }
}

struct CommentSyntax {
    line_prefix: Option<&'static str>,
    block_start: Option<&'static str>,
    block_end: Option<&'static str>,
    doc_prefix: Option<&'static str>,
}

impl CommentSyntax {
    fn from_lang(lang: Option<&str>) -> Self {
        match lang.unwrap_or("") {
            "python" => Self {
                line_prefix: Some("#"),
                block_start: None,
                block_end: None,
                doc_prefix: None,
            },
            "ruby" | "shell" | "bash" => Self {
                line_prefix: Some("#"),
                block_start: None,
                block_end: None,
                doc_prefix: None,
            },
            _ => Self {
                line_prefix: Some("//"),
                block_start: Some("/*"),
                block_end: Some("*/"),
                doc_prefix: Some("///"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_line_comments() {
        let code = "// comment\nfn main() {\n    println!(\"hello\");\n}\n";
        let result = strip_comments(code, Some("rust"));
        assert!(!result.contains("// comment"));
        assert!(result.contains("fn main()"));
    }

    #[test]
    fn test_keep_doc_comments() {
        let code = "/// Doc comment\nfn main() {}\n";
        let result = strip_comments(code, Some("rust"));
        assert!(result.contains("/// Doc comment"));
    }

    #[test]
    fn test_strip_block_comments() {
        let code = "/* block */\nfn main() {}\n";
        let result = strip_comments(code, Some("rust"));
        assert!(!result.contains("block"));
        assert!(result.contains("fn main()"));
    }

    #[test]
    fn test_strip_python_comments() {
        let code = "# comment\ndef main():\n    pass\n";
        let result = strip_comments(code, Some("python"));
        assert!(!result.contains("# comment"));
        assert!(result.contains("def main():"));
    }

    #[test]
    fn test_collapse_blank_lines() {
        let code = "a\n\n\n\n\nb\n";
        let result = strip_comments(code, None);
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn test_smart_strip_keeps_signatures() {
        let code =
            "// comment\nfn main() {\n    let x = 1;\n    let y = 2;\n    println!(x + y);\n}\n";
        let result = smart_strip(code, Some("rust"));
        assert!(result.contains("fn main()"));
        assert!(result.contains("[..."));
        assert!(!result.contains("let x"));
    }

    #[test]
    fn test_smart_strip_keeps_imports() {
        let code = "use std::io;\nimport foo from 'bar';\nfn main() {\n    body();\n}\n";
        let result = smart_strip(code, Some("rust"));
        assert!(result.contains("use std::io;"));
        assert!(result.contains("import foo"));
        assert!(result.contains("fn main()"));
    }
}
