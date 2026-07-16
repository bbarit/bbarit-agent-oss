use once_cell::sync::Lazy;
use regex::Regex;

static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}_][\p{L}\p{N}_]*").unwrap());

fn split_camel_case(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut start = 0;

    for i in 1..chars.len() {
        let should_split = (chars[i - 1].is_lowercase() && chars[i].is_uppercase())
            || (i + 1 < chars.len()
                && chars[i - 1].is_uppercase()
                && chars[i].is_uppercase()
                && chars[i + 1].is_lowercase())
            || (chars[i - 1].is_alphabetic() && chars[i].is_ascii_digit())
            || (chars[i - 1].is_ascii_digit() && chars[i].is_alphabetic());

        if should_split {
            let part: String = chars[start..i].iter().collect();
            if !part.is_empty() {
                parts.push(part.to_lowercase());
            }
            start = i;
        }
    }

    if start < chars.len() {
        let part: String = chars[start..].iter().collect();
        if !part.is_empty() {
            parts.push(part.to_lowercase());
        }
    }

    parts
}

pub fn split_identifier(token: &str) -> Vec<String> {
    let lower = token.to_lowercase();
    let parts: Vec<String> = if token.contains('_') {
        lower
            .split('_')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    } else {
        split_camel_case(token)
    };

    if parts.len() >= 2 {
        let mut result = vec![lower];
        result.extend(parts);
        result
    } else {
        vec![lower]
    }
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    for mat in TOKEN_RE.find_iter(text) {
        result.extend(split_identifier(mat.as_str()));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_identifier_snake_case() {
        assert_eq!(split_identifier("my_func"), vec!["my_func", "my", "func"]);
    }

    #[test]
    fn test_split_identifier_camel_case() {
        assert_eq!(
            split_identifier("HandlerStack"),
            vec!["handlerstack", "handler", "stack"]
        );
    }

    #[test]
    fn test_split_identifier_simple() {
        assert_eq!(split_identifier("simple"), vec!["simple"]);
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("my_func getFoo");
        assert!(tokens.contains(&"my_func".to_string()));
        assert!(tokens.contains(&"my".to_string()));
        assert!(tokens.contains(&"func".to_string()));
        assert!(tokens.contains(&"getfoo".to_string()));
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"foo".to_string()));
    }

    #[test]
    fn test_tokenize_korean() {
        let tokens = tokenize("의존성 그래프 구축");
        assert!(tokens.contains(&"의존성".to_string()));
        assert!(tokens.contains(&"그래프".to_string()));
        assert!(tokens.contains(&"구축".to_string()));
    }

    #[test]
    fn test_tokenize_mixed_korean_english() {
        let tokens = tokenize("Tree-sitter AST 기반 청킹");
        assert!(tokens.contains(&"tree".to_string()));
        assert!(tokens.contains(&"sitter".to_string()));
        assert!(tokens.contains(&"기반".to_string()));
        assert!(tokens.contains(&"청킹".to_string()));
    }
}
