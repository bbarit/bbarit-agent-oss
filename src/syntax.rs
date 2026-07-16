//! Syntect-backed syntax highlighting for the TUI: code fences in assistant
//! markdown and file content in tool cards (read output after the anchor
//! gutter). Stateful per block — a `Highlighter` carries syntect's parse
//! state across lines so multi-line strings/comments color correctly.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let mut themes = ThemeSet::load_defaults();
        // base16-ocean.dark reads well on both the TUI's dark cards and most
        // terminal themes; colors come through as 24-bit RGB spans.
        themes
            .themes
            .remove("base16-ocean.dark")
            .or_else(|| themes.themes.into_values().next())
            .expect("syntect default themes present")
    })
}

/// Find a syntax by a hint: a fence token ("rust", "ts"), a file name
/// ("main.rs"), or an extension ("rs").
fn find_syntax(hint: &str) -> Option<&'static SyntaxReference> {
    let set = syntax_set();
    let hint = hint.trim();
    if hint.is_empty() {
        return None;
    }
    let by_token = set.find_syntax_by_token(hint);
    if by_token.is_some() {
        return by_token;
    }
    let ext = hint.rsplit('.').next().unwrap_or(hint);
    set.find_syntax_by_extension(ext)
}

/// A stateful per-block highlighter. `None` syntax = pass lines through plain.
pub struct Highlighter {
    inner: Option<HighlightLines<'static>>,
}

impl Highlighter {
    /// `hint` may be a fence language, a path, or an extension. Unknown hints
    /// produce a pass-through highlighter (plain text, never an error).
    pub fn new(hint: &str) -> Self {
        Self {
            inner: find_syntax(hint).map(|syntax| HighlightLines::new(syntax, theme())),
        }
    }

    pub fn is_active(&self) -> bool {
        self.inner.is_some()
    }

    /// Highlight one line into ratatui spans (fg RGB from the theme; bold and
    /// italics preserved). Falls back to a single plain span on any failure.
    pub fn line(&mut self, line: &str) -> Vec<Span<'static>> {
        let Some(inner) = self.inner.as_mut() else {
            return vec![Span::styled(
                line.to_string(),
                Style::new().fg(Color::Reset),
            )];
        };
        match inner.highlight_line(line, syntax_set()) {
            Ok(regions) => regions
                .into_iter()
                .map(|(style, text)| {
                    let mut out = Style::new().fg(Color::Rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    ));
                    if style.font_style.contains(FontStyle::BOLD) {
                        out = out.add_modifier(Modifier::BOLD);
                    }
                    if style.font_style.contains(FontStyle::ITALIC) {
                        out = out.add_modifier(Modifier::ITALIC);
                    }
                    Span::styled(text.to_string(), out)
                })
                .collect(),
            Err(_) => vec![Span::styled(
                line.to_string(),
                Style::new().fg(Color::Reset),
            )],
        }
    }
}

/// Word-level diff for an adjacent `- old` / `+ new` pair: the common prefix
/// and suffix stay plainly red/green, the differing middle is REVERSED so the
/// exact change pops (word-level inverse). Returns (old_spans, new_spans) for
/// the content after the -/+ marker.
pub fn intra_line_diff(old: &str, new: &str) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let mut prefix = 0usize;
    while prefix < old_chars.len()
        && prefix < new_chars.len()
        && old_chars[prefix] == new_chars[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < old_chars.len() - prefix
        && suffix < new_chars.len() - prefix
        && old_chars[old_chars.len() - 1 - suffix] == new_chars[new_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let build = |chars: &[char], color: Color| -> Vec<Span<'static>> {
        let base = Style::new().fg(color);
        let head: String = chars[..prefix].iter().collect();
        let mid: String = chars[prefix..chars.len() - suffix].iter().collect();
        let tail: String = chars[chars.len() - suffix..].iter().collect();
        let mut spans = Vec::new();
        if !head.is_empty() {
            spans.push(Span::styled(head, base));
        }
        if !mid.is_empty() {
            spans.push(Span::styled(mid, base.add_modifier(Modifier::REVERSED)));
        }
        if !tail.is_empty() {
            spans.push(Span::styled(tail, base));
        }
        if spans.is_empty() {
            spans.push(Span::styled(String::new(), base));
        }
        spans
    };
    (
        build(&old_chars, Color::Red),
        build(&new_chars, Color::Green),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighter_colors_rust_keywords() {
        let mut hl = Highlighter::new("rs");
        assert!(hl.is_active());
        let spans = hl.line("fn main() { let x = \"hi\"; }");
        // Multiple styled regions — not one flat span.
        assert!(spans.len() > 2, "expected several regions, got {spans:?}");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "fn main() { let x = \"hi\"; }");
    }

    #[test]
    fn highlighter_unknown_language_passes_through() {
        let mut hl = Highlighter::new("definitely-not-a-language");
        assert!(!hl.is_active());
        let spans = hl.line("plain text");
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn intra_line_diff_reverses_only_the_changed_middle() {
        let (old, new) = intra_line_diff("let count = 1;", "let count = 2;");
        let old_text: String = old.iter().map(|s| s.content.as_ref()).collect();
        let new_text: String = new.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(old_text, "let count = 1;");
        assert_eq!(new_text, "let count = 2;");
        // Middle span is the changed char, reversed.
        let old_mid = old
            .iter()
            .find(|s| s.style.add_modifier.contains(Modifier::REVERSED))
            .expect("reversed span");
        assert_eq!(old_mid.content.as_ref(), "1");
        let new_mid = new
            .iter()
            .find(|s| s.style.add_modifier.contains(Modifier::REVERSED))
            .expect("reversed span");
        assert_eq!(new_mid.content.as_ref(), "2");
    }
}
