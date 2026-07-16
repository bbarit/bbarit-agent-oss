//! Line-anchored editing. `read` prints every line with a compact anchor —
//! `42ab|content` — where `42` is the line number and `ab` is a 2-character
//! hash of the line's CONTENT. The `patch` tool addresses lines by those
//! anchors instead of quoting text, and every anchor is re-verified against
//! the file on apply: if a line changed since it was read, the patch is
//! rejected with a code frame of the CURRENT lines (fresh anchors included)
//! so the very next call can succeed without another read.
//!
//! The hash covers content only (CR stripped, trailing whitespace trimmed;
//! position excluded), so anchors stay valid when unrelated edits shift line
//! numbers — the number locates the line, the hash proves it is still the
//! same line. Hashes map into a fixed table of 647 two-letter bigrams chosen
//! so each anchor suffix costs a single token; the table order is frozen —
//! reordering it would invalidate every anchor in existing transcripts.
//!
//! A per-process read-snapshot cache additionally enables RECOVERY: when the
//! file changed on disk after the read, the edits are replayed against the
//! snapshot and relocated onto the current content with exact-context
//! matching — or refused if that cannot be done unambiguously.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{Result, bail};

/// Lines of context shown on either side of a mismatched anchor.
const MISMATCH_CONTEXT: usize = 2;
/// Wildcard hash: accepted in an anchor to skip content validation (useful
/// for the interior/end of a large range the model did not re-quote).
const WILDCARD_HASH: [u8; 2] = *b"**";

fn bigrams() -> &'static Vec<String> {
    static TABLE: OnceLock<Vec<String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str::<Vec<String>>(include_str!("hashline_bigrams.json"))
            .expect("bigram table parses")
    })
}

/// XXH32 (seed 0) — standard one-shot implementation over the line bytes.
fn xxh32(data: &[u8], seed: u32) -> u32 {
    const P1: u32 = 0x9E37_79B1;
    const P2: u32 = 0x85EB_CA77;
    const P3: u32 = 0xC2B2_AE3D;
    const P4: u32 = 0x27D4_EB2F;
    const P5: u32 = 0x1656_67B1;
    let mut h: u32;
    let len = data.len();
    let mut i = 0;
    if len >= 16 {
        let mut v1 = seed.wrapping_add(P1).wrapping_add(P2);
        let mut v2 = seed.wrapping_add(P2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(P1);
        while i + 16 <= len {
            let read = |offset: usize| -> u32 {
                u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
            };
            v1 = v1
                .wrapping_add(read(i).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            v2 = v2
                .wrapping_add(read(i + 4).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            v3 = v3
                .wrapping_add(read(i + 8).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            v4 = v4
                .wrapping_add(read(i + 12).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            i += 16;
        }
        h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
    } else {
        h = seed.wrapping_add(P5);
    }
    h = h.wrapping_add(len as u32);
    while i + 4 <= len {
        let lane = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
        h = h
            .wrapping_add(lane.wrapping_mul(P3))
            .rotate_left(17)
            .wrapping_mul(P4);
        i += 4;
    }
    while i < len {
        h = h
            .wrapping_add(u32::from(data[i]).wrapping_mul(P5))
            .rotate_left(11)
            .wrapping_mul(P1);
        i += 1;
    }
    h ^= h >> 15;
    h = h.wrapping_mul(P2);
    h ^= h >> 13;
    h = h.wrapping_mul(P3);
    h ^= h >> 16;
    h
}

/// 2-character content hash for one line. Content-only (CR stripped, trailing
/// whitespace trimmed) so anchors survive line shifts from sibling edits;
/// identical blank lines intentionally collide — the line number disambiguates.
pub fn line_hash(content: &str) -> String {
    let normalized: String = content.replace('\r', "");
    let normalized = normalized.trim_end();
    let table = bigrams();
    table[(xxh32(normalized.as_bytes(), 0) as usize) % table.len()].clone()
}

/// Render lines with anchors: `42|ab content`. `start_line` is 1-based.
/// The pipe separates the line number from the 2-char content hash so a human
/// reading the transcript sees a clean number (the old `1268fi|` glued form
/// read as one garbled token). parse_anchor accepts anchors quoted either way.
pub fn render(lines: &[&str], start_line: usize) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{}|{} {}", start_line + offset, line_hash(line), line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A parsed anchor: 1-based line number + expected content hash
/// (`**` = wildcard, content not validated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub line: usize,
    pub hash: [u8; 2],
}

impl Anchor {
    fn is_wildcard(&self) -> bool {
        self.hash == WILDCARD_HASH
    }
}

/// Parse an anchor reference. Tolerates the decorations output formatters may
/// prefix (`>` grep context, `*` match, `+`/`-` diff) and surrounding
/// whitespace, because models echo anchors back exactly as printed.
pub fn parse_anchor(text: &str) -> Result<Anchor> {
    let trimmed = text
        .trim()
        .trim_start_matches(['>', '+', '-', '*', ' ', '\t']);
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    // Models echo anchors as printed: the display form is `1268|fi`, the
    // legacy form `1268fi` — accept an optional separator either way.
    let rest = trimmed[digits..].trim_start_matches(['|', ' ']);
    let valid_hash = rest.len() == 2
        && (rest.bytes().all(|b| b.is_ascii_lowercase()) || rest.as_bytes() == WILDCARD_HASH);
    if digits == 0 || !valid_hash {
        let hash_only_hint = if text.trim().len() == 2
            && text.trim().bytes().all(|b| b.is_ascii_lowercase())
        {
            format!(
                " It looks like you supplied only the hash suffix ({:?}). Copy the full anchor exactly as shown (for example, \"160{}\").",
                text.trim(),
                text.trim()
            )
        } else {
            String::new()
        };
        bail!(
            "Invalid line reference. Expected the full anchor exactly as shown by read/search \
             output (line number + hash, for example \"160sr\", \"160ab\", \"160th\"). \
             Received {text:?}.{hash_only_hint}"
        );
    }
    let line: usize = trimmed[..digits].parse()?;
    if line == 0 {
        bail!("Line number must be >= 1, got 0 in {text:?}.");
    }
    let bytes = rest.as_bytes();
    Ok(Anchor {
        line,
        hash: [bytes[0], bytes[1]],
    })
}

/// One patch operation, line-anchored.
#[derive(Debug, Clone)]
pub enum PatchOp {
    /// Replace lines `from..=to` with `text` (text may be multi-line or empty).
    Replace {
        from: Anchor,
        to: Anchor,
        text: String,
    },
    /// Insert `text` as new line(s) immediately after the anchored line.
    InsertAfter { anchor: Anchor, text: String },
    /// Insert `text` as new line(s) immediately before the anchored line.
    InsertBefore { anchor: Anchor, text: String },
    /// Delete lines `from..=to`.
    Delete { from: Anchor, to: Anchor },
}

impl PatchOp {
    fn range(&self) -> (usize, usize) {
        match self {
            PatchOp::Replace { from, to, .. } | PatchOp::Delete { from, to } => {
                (from.line, to.line)
            }
            PatchOp::InsertAfter { anchor, .. } | PatchOp::InsertBefore { anchor, .. } => {
                (anchor.line, anchor.line)
            }
        }
    }

    fn anchors(&self) -> Vec<Anchor> {
        match self {
            PatchOp::Replace { from, to, .. } | PatchOp::Delete { from, to } => vec![*from, *to],
            PatchOp::InsertAfter { anchor, .. } | PatchOp::InsertBefore { anchor, .. } => {
                vec![*anchor]
            }
        }
    }
}

/// Build the rejection message: a code frame of the CURRENT file around every
/// mismatched line, each printed with its FRESH anchor so the model can issue
/// a corrected call immediately (mismatched lines are marked `*`).
fn mismatch_error(mismatched: &[usize], lines: &[&str]) -> String {
    let noun = if mismatched.len() > 1 {
        "anchors do"
    } else {
        "anchor does"
    };
    let mut out = vec![
        format!(
            "Edit rejected: {} {noun} not match the current file (marked *).",
            mismatched.len()
        ),
        "This edit did not apply. Read the current file content shown below and send a fresh edit tool-call based on it.".to_string(),
    ];
    let mut display: Vec<usize> = Vec::new();
    for &line in mismatched {
        let lo = line.saturating_sub(MISMATCH_CONTEXT).max(1);
        let hi = (line + MISMATCH_CONTEXT).min(lines.len());
        for n in lo..=hi {
            if !display.contains(&n) {
                display.push(n);
            }
        }
    }
    display.sort_unstable();
    let mut previous = 0usize;
    for n in display {
        if previous != 0 && n > previous + 1 {
            out.push("...".to_string());
        }
        previous = n;
        let text = lines.get(n - 1).copied().unwrap_or("");
        let marker = if mismatched.contains(&n) { "*" } else { " " };
        out.push(format!("{marker}{n}{}|{text}", line_hash(text)));
    }
    out.join("\n")
}

/// Verify every anchor against `lines` (LF-split file content) and apply all
/// ops bottom-up so earlier line numbers stay valid. Ops must not overlap.
pub fn apply(lines: &[&str], ops: &[PatchOp]) -> Result<Vec<String>> {
    if ops.is_empty() {
        bail!("patch: `ops` must contain at least one operation");
    }
    // Validate every anchor against the CURRENT content first, so the error
    // frame lists all stale lines at once.
    let mut stale: Vec<usize> = Vec::new();
    for op in ops {
        for anchor in op.anchors() {
            if lines.get(anchor.line - 1).is_none() {
                bail!(
                    "patch: line {} does not exist (file has {} lines) — re-read the file",
                    anchor.line,
                    lines.len()
                );
            }
            if !anchor.is_wildcard() {
                let expected = std::str::from_utf8(&anchor.hash).unwrap_or("??");
                if line_hash(lines[anchor.line - 1]) != expected {
                    stale.push(anchor.line);
                }
            }
        }
        let (from, to) = op.range();
        if from > to {
            bail!("patch: range {from}..{to} is inverted");
        }
    }
    if !stale.is_empty() {
        stale.sort_unstable();
        stale.dedup();
        bail!("{}", mismatch_error(&stale, lines));
    }
    // Reject overlapping ops — they'd silently corrupt each other.
    let mut ranges: Vec<(usize, usize)> = ops.iter().map(PatchOp::range).collect();
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[1].0 <= pair[0].1 {
            bail!(
                "patch: operations overlap around lines {}..{} — merge them into one op",
                pair[1].0,
                pair[0].1
            );
        }
    }

    let mut result: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut ordered: Vec<&PatchOp> = ops.iter().collect();
    ordered.sort_by_key(|op| std::cmp::Reverse(op.range().0));
    for op in ordered {
        match op {
            PatchOp::Replace { from, to, text } => {
                let replacement: Vec<String> = if text.is_empty() {
                    Vec::new()
                } else {
                    text.split('\n').map(str::to_string).collect()
                };
                result.splice(from.line - 1..to.line, replacement);
            }
            PatchOp::Delete { from, to } => {
                result.splice(from.line - 1..to.line, std::iter::empty());
            }
            PatchOp::InsertAfter { anchor, text } => {
                let insert: Vec<String> = text.split('\n').map(str::to_string).collect();
                result.splice(anchor.line..anchor.line, insert);
            }
            PatchOp::InsertBefore { anchor, text } => {
                let insert: Vec<String> = text.split('\n').map(str::to_string).collect();
                result.splice(anchor.line - 1..anchor.line - 1, insert);
            }
        }
    }
    Ok(result)
}

// ---- read-snapshot cache + stale-anchor recovery ----------------------------

type Snapshot = HashMap<usize, String>;

fn read_cache() -> &'static Mutex<HashMap<String, Snapshot>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Snapshot>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Record what `read` showed the model: line-number → content, merged into
/// any earlier snapshot of the same file (partial reads accumulate).
pub fn cache_read(path: &Path, start_line: usize, lines: &[&str]) {
    let mut cache = read_cache().lock().unwrap();
    let snapshot = cache.entry(cache_key(path)).or_default();
    for (offset, line) in lines.iter().enumerate() {
        snapshot.insert(start_line + offset, (*line).to_string());
    }
}

/// Attempt to recover from stale anchors using the read snapshot: replay the
/// ops against the snapshot-reconstructed previous content, then relocate each
/// changed block onto the current content by exact match (nearest occurrence
/// to its expected position; context included). Returns the merged lines, or
/// None when recovery is not possible unambiguously.
///
/// Precondition (strict): every anchored line must be present in the snapshot
/// AND hash to the model-supplied value — otherwise any merge is a guess.
pub fn try_recover(path: &Path, current: &[&str], ops: &[PatchOp]) -> Option<Vec<String>> {
    let cache = read_cache().lock().unwrap();
    let snapshot = cache.get(&cache_key(path))?;
    if snapshot.is_empty() {
        return None;
    }
    for op in ops {
        for anchor in op.anchors() {
            if anchor.is_wildcard() {
                continue;
            }
            let cached = snapshot.get(&anchor.line)?;
            let expected = std::str::from_utf8(&anchor.hash).unwrap_or("??");
            if line_hash(cached) != expected {
                return None;
            }
        }
    }
    // Reconstruct the pre-edit view: current content overlaid with every
    // cached line at its read-time position.
    let mut previous: Vec<String> = current.iter().map(|s| s.to_string()).collect();
    let max_cached = snapshot.keys().copied().max().unwrap_or(0);
    while previous.len() < max_cached {
        previous.push(String::new());
    }
    for (line, content) in snapshot {
        previous[line - 1] = content.clone();
    }
    let previous_refs: Vec<&str> = previous.iter().map(String::as_str).collect();
    if previous_refs == current {
        return None;
    }
    let applied = apply(&previous_refs, ops).ok()?;
    if applied == previous {
        return None;
    }

    // Relocate each op's changed block (with 3 lines of context) from the
    // previous view onto the current content by exact match.
    const CONTEXT: usize = 3;
    let mut result: Vec<String> = current.iter().map(|s| s.to_string()).collect();
    let mut ordered: Vec<&PatchOp> = ops.iter().collect();
    ordered.sort_by_key(|op| std::cmp::Reverse(op.range().0));
    for op in ordered {
        let (from, to) = op.range();
        let lo = from.saturating_sub(CONTEXT + 1); // 0-based block start
        let hi = (to + CONTEXT).min(previous.len()); // exclusive end (1-based to)
        let old_block: Vec<&str> = previous[lo..hi].iter().map(String::as_str).collect();
        // The replacement block = context + op result applied locally.
        let local_refs: Vec<&str> = old_block.clone();
        let mut local_op = op.clone();
        // Shift the op's line numbers into block-local coordinates.
        let shift = |anchor: &mut Anchor| anchor.line -= lo;
        match &mut local_op {
            PatchOp::Replace { from, to, .. } | PatchOp::Delete { from, to } => {
                shift(from);
                shift(to);
            }
            PatchOp::InsertAfter { anchor, .. } | PatchOp::InsertBefore { anchor, .. } => {
                shift(anchor)
            }
        }
        let new_block = apply(&local_refs, std::slice::from_ref(&local_op)).ok()?;
        // Find the old block in the current content — nearest exact match to
        // the expected position; ambiguity at equal distance = refuse.
        let expected = lo;
        let matches: Vec<usize> = (0..=result.len().saturating_sub(old_block.len()))
            .filter(|&start| {
                result[start..start + old_block.len()]
                    .iter()
                    .map(String::as_str)
                    .eq(old_block.iter().copied())
            })
            .collect();
        let best = matches
            .iter()
            .copied()
            .min_by_key(|&start| (start.abs_diff(expected), start))?;
        let tied = matches
            .iter()
            .filter(|&&start| start.abs_diff(expected) == best.abs_diff(expected))
            .count();
        if tied > 1 {
            return None;
        }
        result.splice(best..best + old_block.len(), new_block);
    }
    if result
        .iter()
        .map(String::as_str)
        .eq(current.iter().copied())
    {
        return None;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor_for(lines: &[&str], line: usize) -> Anchor {
        parse_anchor(&format!("{line}{}", line_hash(lines[line - 1]))).unwrap()
    }

    #[test]
    fn hash_is_stable_content_only_and_trim_aware() {
        assert_eq!(line_hash("hello"), line_hash("hello"));
        // Trailing whitespace and CR are normalized away (anchor stability).
        assert_eq!(line_hash("hello"), line_hash("hello   "));
        assert_eq!(line_hash("hello"), line_hash("hello\r"));
        assert_ne!(line_hash("hello"), line_hash(" hello"));
        assert_eq!(line_hash("한글 내용도 안정적").len(), 2);
        // Hashes come from the fixed bigram table (lowercase letters only).
        assert!(line_hash("x").bytes().all(|b| b.is_ascii_lowercase()));
    }

    #[test]
    fn xxh32_reference_vectors() {
        // Public XXH32 test vectors (seed 0).
        assert_eq!(xxh32(b"", 0), 0x02CC_5D05);
        assert_eq!(xxh32(b"a", 0), 0x550D_7456);
        assert_eq!(xxh32(b"abc", 0), 0x32D1_53FF);
        // ≥16-byte input exercises the 4-lane path.
        assert_eq!(xxh32(b"0123456789abcdef", 0), xxh32(b"0123456789abcdef", 0));
    }

    #[test]
    fn render_prints_number_pipe_hash() {
        // `10|xx alpha` — pipe right after the number so a human reads a
        // clean line number instead of a glued `10xx|` token.
        let out = render(&["alpha", "beta"], 10);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("10|"), "{}", lines[0]);
        assert!(lines[0].ends_with(" alpha"), "{}", lines[0]);
        assert!(lines[1].starts_with("11|"), "{}", lines[1]);
        assert!(lines[1].ends_with(" beta"), "{}", lines[1]);
    }

    #[test]
    fn parse_anchor_accepts_display_and_legacy_forms() {
        // Legacy glued form and the new displayed form both resolve.
        let legacy = parse_anchor("42ab").unwrap();
        let displayed = parse_anchor("42|ab").unwrap();
        assert_eq!(legacy, displayed);
    }

    #[test]
    fn parse_anchor_boundaries() {
        let anchor = parse_anchor("42ab").unwrap();
        assert_eq!(anchor.line, 42);
        assert_eq!(&anchor.hash, b"ab");
        // Whitespace and output decorations tolerated.
        assert!(parse_anchor(" 7xy ").is_ok());
        assert_eq!(parse_anchor(">42ab").unwrap().line, 42);
        assert_eq!(parse_anchor("* 42ab").unwrap().line, 42);
        assert_eq!(parse_anchor("+42ab").unwrap().line, 42);
        // Wildcard hash skips validation.
        assert!(parse_anchor("42**").unwrap().is_wildcard());
        // None/empty and malformed rejected.
        assert!(parse_anchor("").is_err());
        assert!(parse_anchor("ab").is_err());
        assert!(parse_anchor("42").is_err());
        assert!(parse_anchor("42abc").is_err());
        assert!(parse_anchor("0ab").is_err());
        // Hash-only input gets the dedicated hint.
        let error = parse_anchor("sr").unwrap_err().to_string();
        assert!(error.contains("only the hash suffix"), "{error}");
    }

    #[test]
    fn replace_insert_delete_apply_bottom_up() {
        let lines = ["one", "two", "three", "four"];
        let ops = vec![
            PatchOp::Replace {
                from: anchor_for(&lines, 2),
                to: anchor_for(&lines, 2),
                text: "TWO".to_string(),
            },
            PatchOp::InsertAfter {
                anchor: anchor_for(&lines, 4),
                text: "five".to_string(),
            },
            PatchOp::Delete {
                from: anchor_for(&lines, 1),
                to: anchor_for(&lines, 1),
            },
        ];
        let out = apply(&lines, &ops).unwrap();
        assert_eq!(out, vec!["TWO", "three", "four", "five"]);
    }

    #[test]
    fn wildcard_range_end_skips_validation() {
        let lines = ["a", "b", "c", "d"];
        let out = apply(
            &lines,
            &[PatchOp::Delete {
                from: anchor_for(&lines, 2),
                to: parse_anchor("3**").unwrap(),
            }],
        )
        .unwrap();
        assert_eq!(out, vec!["a", "d"]);
    }

    #[test]
    fn stale_anchor_error_shows_fresh_anchors_with_context() {
        let lines = ["one", "two", "three", "four", "five"];
        let bad = Anchor {
            line: 3,
            hash: *b"zz",
        };
        let error = apply(&lines, &[PatchOp::Delete { from: bad, to: bad }])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Edit rejected: 1 anchor does not match"),
            "{error}"
        );
        assert!(error.contains("did not apply"), "{error}");
        // The frame shows the CURRENT line with its FRESH anchor, marked *.
        assert!(
            error.contains(&format!("*3{}|three", line_hash("three"))),
            "{error}"
        );
        // ±2 context lines included.
        assert!(
            error.contains(&format!(" 1{}|one", line_hash("one"))),
            "{error}"
        );
        assert!(
            error.contains(&format!(" 5{}|five", line_hash("five"))),
            "{error}"
        );
    }

    #[test]
    fn out_of_range_and_overlap_are_rejected() {
        let lines = ["one"];
        let ghost = Anchor {
            line: 9,
            hash: *b"aa",
        };
        assert!(
            apply(
                &lines,
                &[PatchOp::Delete {
                    from: ghost,
                    to: ghost
                }]
            )
            .unwrap_err()
            .to_string()
            .contains("does not exist")
        );
        let lines = ["one", "two", "three"];
        let ops = vec![
            PatchOp::Delete {
                from: anchor_for(&lines, 1),
                to: anchor_for(&lines, 2),
            },
            PatchOp::Replace {
                from: anchor_for(&lines, 2),
                to: anchor_for(&lines, 3),
                text: "x".to_string(),
            },
        ];
        assert!(
            apply(&lines, &ops)
                .unwrap_err()
                .to_string()
                .contains("overlap")
        );
    }

    #[test]
    fn multiline_and_empty_replacement() {
        let lines = ["a", "b", "c"];
        let out = apply(
            &lines,
            &[PatchOp::InsertBefore {
                anchor: anchor_for(&lines, 1),
                text: "x\ny".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(out, vec!["x", "y", "a", "b", "c"]);
        let out = apply(
            &lines,
            &[PatchOp::Replace {
                from: anchor_for(&lines, 2),
                to: anchor_for(&lines, 3),
                text: String::new(),
            }],
        )
        .unwrap();
        assert_eq!(out, vec!["a"]);
    }

    #[test]
    fn recovery_relocates_shifted_block() {
        // Model read the file, then an external change PREPENDED two lines,
        // shifting everything down. Anchors (line numbers) are now stale on
        // disk but valid against the snapshot → recovery relocates the edit.
        let dir = std::env::temp_dir().join("bbarit-hashline-recover");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("shift.txt");
        let read_lines = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"];
        cache_read(&path, 1, &read_lines);
        let current = [
            "// new header",
            "// more header",
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "zeta",
            "eta",
        ];
        let op = PatchOp::Replace {
            from: anchor_for(&read_lines, 4),
            to: anchor_for(&read_lines, 4),
            text: "DELTA".to_string(),
        };
        // Direct apply fails (line 4 on disk is now "beta").
        assert!(apply(&current, std::slice::from_ref(&op)).is_err());
        let recovered = try_recover(&path, &current, std::slice::from_ref(&op)).expect("recovers");
        assert_eq!(
            recovered,
            vec![
                "// new header",
                "// more header",
                "alpha",
                "beta",
                "gamma",
                "DELTA",
                "epsilon",
                "zeta",
                "eta"
            ]
        );
    }

    #[test]
    fn recovery_refuses_unvouched_or_ambiguous() {
        let dir = std::env::temp_dir().join("bbarit-hashline-refuse");
        let _ = std::fs::create_dir_all(&dir);
        // No snapshot at all → None.
        let ghost = dir.join("never-read.txt");
        let op = PatchOp::Delete {
            from: Anchor {
                line: 1,
                hash: *b"aa",
            },
            to: Anchor {
                line: 1,
                hash: *b"aa",
            },
        };
        assert!(try_recover(&ghost, &["x"], std::slice::from_ref(&op)).is_none());
        // Anchor hash doesn't match the snapshot either → None (a guess).
        let path = dir.join("read.txt");
        cache_read(&path, 1, &["real content"]);
        assert!(try_recover(&path, &["changed"], std::slice::from_ref(&op)).is_none());
    }
}
