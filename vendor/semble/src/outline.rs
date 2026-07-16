use once_cell::sync::Lazy;
use regex::Regex;

static SIGNATURE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"^\s*",
        r"(?:pub(?:\([^)]*\))?\s+|export\s+(?:default\s+)?|public\s+|private\s+|protected\s+|",
        r"static\s+|abstract\s+|final\s+|async\s+|const\s+|unsafe\s+|extern\s+)*",
        r"(?:fn|function|def|class|struct|enum|trait|impl|interface|type|mod|func|fun|",
        r"namespace|package|protocol|record|typedef|module|defmodule|object)\b",
    ))
    .unwrap()
});

static ARROW_FN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\s*(?:export\s+(?:default\s+)?)?(?:const|let|var)\s+[A-Za-z_$][\w$]*\s*=\s*(?:async\s+)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>",
    )
    .unwrap()
});

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with("*") || t.starts_with("#")
}

fn is_definition_line(line: &str) -> bool {
    SIGNATURE_RE.is_match(line) || ARROW_FN_RE.is_match(line)
}

/// Extract a signature line for an entire chunk.
///
/// Picks the first definition line in the chunk. Multi-line signatures
/// (`fn foo(\n    arg: T,\n) -> R {`) are joined into one line up to the
/// terminating `{`, `;`, or `=>`.
pub fn extract_signature(content: &str) -> Option<String> {
    extract_signature_near(content, 1, &[])
}

/// Extract a signature line, preferring the definition closest *above* the
/// first match line.
///
/// `chunk_start_line` is the 1-indexed line number where this chunk begins in
/// the source file. `match_lines` is a list of absolute line numbers where
/// the search query matched. If a match exists inside the chunk, the chosen
/// definition is the nearest `is_definition_line` that occurs at or before
/// the first match line — so when a chunk contains multiple definitions, the
/// one whose body actually contains the match is preferred.
pub fn extract_signature_near(
    content: &str,
    chunk_start_line: usize,
    match_lines: &[usize],
) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut def_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() || is_comment_line(line) {
            continue;
        }
        if is_definition_line(line) {
            def_indices.push(i);
        }
    }

    if def_indices.is_empty() {
        for line in lines.iter() {
            let trimmed = line.trim();
            if trimmed.is_empty() || is_comment_line(line) {
                continue;
            }
            return Some(format_signature(trimmed));
        }
        return None;
    }

    let chosen = if let Some(&first_match_abs) = match_lines.first() {
        let match_rel = first_match_abs.saturating_sub(chunk_start_line);
        def_indices
            .iter()
            .rev()
            .copied()
            .find(|&i| i <= match_rel)
            .unwrap_or(def_indices[0])
    } else {
        def_indices[0]
    };

    Some(collect_signature(&lines, chosen))
}

/// Join lines starting at `start` into a complete signature.
///
/// Tracks paren/bracket depth so multi-line signatures (`fn foo(\n    a,\n    b,\n) -> R {`)
/// are joined until all parens are closed AND a terminator (`{`, `;`, `=>`) is reached.
/// Bounded at 20 lines for safety on pathological inputs.
fn collect_signature(lines: &[&str], start: usize) -> String {
    let mut combined = String::new();
    let mut paren_depth: i32 = 0;
    let mut in_string = false;
    let mut prev: char = ' ';

    let mut count = 0;
    for line in lines.iter().skip(start) {
        if count >= 20 {
            break;
        }
        count += 1;

        let trimmed = line.trim();
        if !combined.is_empty() {
            combined.push(' ');
        }
        combined.push_str(trimmed);

        for ch in trimmed.chars() {
            if in_string {
                if ch == '"' && prev != '\\' {
                    in_string = false;
                }
            } else {
                match ch {
                    '"' => in_string = true,
                    '(' | '[' => paren_depth += 1,
                    ')' | ']' => paren_depth -= 1,
                    _ => {}
                }
            }
            prev = ch;
        }

        if paren_depth <= 0
            && (trimmed.ends_with('{')
                || trimmed.ends_with(';')
                || trimmed.ends_with("=>")
                || trimmed.contains("=>")
                || trimmed.ends_with(':'))
        {
            break;
        }
        // 함수 시그니처가 한 줄에 완성된 경우 (paren 닫혔고 다른 종결자 없어도 OK)
        if paren_depth <= 0 && count == 1 && (trimmed.contains("->") || trimmed.ends_with(')')) {
            // 더 합칠 라인이 없을 수 있으니 다음 라인을 한 번만 더 보고 break
            // (e.g., `pub fn foo(x: T)` 다음 줄이 `-> R {` 일 수 있음)
            continue;
        }
        if paren_depth <= 0 && count > 1 && !combined.trim_end().ends_with(',') {
            break;
        }
    }
    format_signature(&combined)
}

fn format_signature(line: &str) -> String {
    let trimmed = line.trim();
    let s = trimmed.trim_end_matches('{').trim_end();
    // Never truncate normal signatures — accuracy first.
    // 500 char ceiling is only a safety net for pathological inputs.
    let max_len = 500;
    if s.chars().count() <= max_len {
        return s.to_string();
    }
    let head: String = s.chars().take(max_len).collect();
    format!("{head}…")
}

/// Quality metric: does this signature look complete?
///
/// A signature is considered "well-formed" if:
/// - parens balance (no orphan `(` or `)`)
/// - it ends with one of: `)`, `;`, `=>`, `:`, `}` (for empty bodies), or an identifier
/// - it isn't the truncation marker `...`
pub fn is_well_formed(sig: &str) -> bool {
    if sig.ends_with("...") {
        return false;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut prev = ' ';
    for ch in sig.chars() {
        if in_string {
            if ch == '"' && prev != '\\' {
                in_string = false;
            }
        } else {
            match ch {
                '"' => in_string = true,
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        prev = ch;
    }
    if depth != 0 {
        return false;
    }
    let last = sig.trim_end().chars().last().unwrap_or(' ');
    matches!(last, ')' | ';' | '}' | ':' | '>') || sig.contains("=>") || last.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_fn_signature() {
        let code = "    pub fn login(email: &str, password: &str) -> Result<User> {\n        todo!()\n    }\n";
        let sig = extract_signature(code).unwrap();
        assert!(sig.contains("pub fn login"));
        assert!(!sig.contains("{"));
    }

    #[test]
    fn rust_multiline_signature_joined() {
        let code = "pub fn rerank_topk(\n    scores: &HashMap<usize, f64>,\n    chunks: &[Chunk],\n    top_k: usize,\n) -> Vec<(usize, f64)> {\n    todo!()\n}\n";
        let sig = extract_signature(code).unwrap();
        assert!(sig.contains("pub fn rerank_topk"));
        assert!(sig.contains("scores"));
        assert!(sig.contains("top_k"));
        assert!(sig.contains("-> Vec<(usize, f64)>"));
        assert!(!sig.contains("todo!"));
    }

    #[test]
    fn ts_export_function() {
        let code = "// header\nexport async function loginWithEmail(email: string, password: string): Promise<User> {\n  return {};\n}\n";
        let sig = extract_signature(code).unwrap();
        assert!(sig.contains("export async function loginWithEmail"));
    }

    #[test]
    fn ts_arrow_const() {
        let code = "export const fetchUser = async (id: string): Promise<User> => {\n  return await api.get(id);\n};\n";
        let sig = extract_signature(code).unwrap();
        assert!(sig.contains("fetchUser"));
        assert!(sig.contains("=>"));
    }

    #[test]
    fn python_def() {
        let code = "# comment\ndef compute_score(query, chunks):\n    return 0.0\n";
        let sig = extract_signature(code).unwrap();
        assert!(sig.starts_with("def compute_score"));
    }

    #[test]
    fn fallback_first_code_line() {
        let code = "// only comments here\nlet x = 42;\n";
        let sig = extract_signature(code).unwrap();
        assert_eq!(sig, "let x = 42;");
    }

    #[test]
    fn picks_definition_near_match_line() {
        // Chunk starts at line 100. Two function definitions: line 100 and line 120.
        // First match is at line 125 — should pick the line-120 definition.
        let mut code = String::new();
        code.push_str("pub fn first_one(a: u32) -> u32 {\n"); // chunk line 0 (abs 100)
        for _ in 0..18 {
            code.push_str("    let _x = 1;\n");
        } // chunk lines 1..=18
        code.push_str("} // close first\n"); // chunk line 19
        code.push_str("pub fn second_one(b: u32) -> u32 {\n"); // chunk line 20 (abs 120)
        for _ in 0..3 {
            code.push_str("    let _y = matching_pattern();\n");
        } // chunk line 21..=23
        code.push_str("}\n");

        let sig = extract_signature_near(&code, 100, &[125]).unwrap();
        assert!(
            sig.contains("second_one"),
            "expected second_one, got: {sig}"
        );
    }

    #[test]
    fn picks_first_definition_when_no_match_lines() {
        let code = "pub fn first(a: u32) -> u32 {\n    todo!()\n}\npub fn second(b: u32) -> u32 {\n    todo!()\n}\n";
        let sig = extract_signature(code).unwrap();
        assert!(sig.contains("first"));
    }

    #[test]
    fn multiline_signature_includes_return_type() {
        let code = "pub fn search_hybrid(\n    query: &str,\n    encoder: &StaticEncoder,\n    semantic_index: &SemanticIndex,\n    bm25_index: &Bm25Index,\n    chunks: &[Chunk],\n    top_k: usize,\n) -> Vec<SearchResult> {\n    todo!()\n}\n";
        let sig = extract_signature(code).unwrap();
        assert!(
            sig.contains("-> Vec<SearchResult>"),
            "missing return type: {sig}"
        );
        assert!(is_well_formed(&sig), "not well-formed: {sig}");
    }

    #[test]
    fn well_formed_recognizes_complete_signatures() {
        assert!(is_well_formed("fn foo(x: u32) -> u32"));
        assert!(is_well_formed("pub async fn bar(a: T, b: U) -> Result<V>"));
        assert!(is_well_formed("const fetchUser = (id) =>"));
        assert!(is_well_formed("class Foo"));
    }

    #[test]
    fn well_formed_rejects_truncated_signatures() {
        assert!(!is_well_formed("fn foo(x: u32,"));
        assert!(!is_well_formed("pub fn bar( a: T, b: U,"));
        assert!(!is_well_formed("fn baz(arg: T) -> R..."));
    }
}
