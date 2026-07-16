use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{CallType, SearchResult};

fn stats_file() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".semble")
        .join("savings.jsonl")
}

#[derive(Serialize, Deserialize)]
struct StatsRecord {
    ts: f64,
    call: String,
    results: usize,
    snippet_chars: usize,
    file_chars: usize,
}

#[derive(Debug, Default)]
pub struct BucketStats {
    pub calls: usize,
    pub snippet_chars: usize,
    pub file_chars: usize,
    pub saved_chars: usize,
}

impl BucketStats {
    fn add(&mut self, snippet_chars: usize, file_chars: usize) {
        self.calls += 1;
        self.snippet_chars += snippet_chars;
        self.file_chars += file_chars;
        self.saved_chars += file_chars.saturating_sub(snippet_chars);
    }
}

pub struct SavingsSummary {
    pub buckets: Vec<(String, BucketStats)>,
    pub call_type_counts: HashMap<String, usize>,
}

pub fn save_search_stats(
    results: &[SearchResult],
    call_type: CallType,
    file_sizes: &HashMap<String, usize>,
) {
    let snippet_chars: usize = results.iter().map(|r| r.chunk.content.len()).sum();
    let unique_files: std::collections::HashSet<&str> =
        results.iter().map(|r| r.chunk.file_path.as_str()).collect();
    let file_chars: usize = unique_files.iter().filter_map(|p| file_sizes.get(*p)).sum();

    let record = StatsRecord {
        ts: Utc::now().timestamp() as f64,
        call: call_type.to_string(),
        results: results.len(),
        snippet_chars,
        file_chars,
    };

    let path = stats_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&record) {
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{json}")
            });
    }
}

pub fn build_savings_summary(path: &Path) -> anyhow::Result<SavingsSummary> {
    let content = fs::read_to_string(path)?;
    let now = Utc::now();
    let today = now.date_naive();
    let seven_days_ago = today - chrono::Duration::days(7);

    let mut today_bucket = BucketStats::default();
    let mut week_bucket = BucketStats::default();
    let mut all_bucket = BucketStats::default();
    let mut call_type_counts: HashMap<String, usize> = HashMap::new();

    for line in content.lines() {
        let record: StatsRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let dt = DateTime::from_timestamp(record.ts as i64, 0)
            .map(|d| d.naive_utc().date())
            .unwrap_or(NaiveDate::MIN);

        *call_type_counts.entry(record.call).or_default() += 1;
        all_bucket.add(record.snippet_chars, record.file_chars);
        if dt > seven_days_ago {
            week_bucket.add(record.snippet_chars, record.file_chars);
        }
        if dt == today {
            today_bucket.add(record.snippet_chars, record.file_chars);
        }
    }

    Ok(SavingsSummary {
        buckets: vec![
            ("Today".to_string(), today_bucket),
            ("Last 7 days".to_string(), week_bucket),
            ("All time".to_string(), all_bucket),
        ],
        call_type_counts,
    })
}

pub fn format_savings_report(verbose: bool) -> String {
    let path = stats_file();
    if !path.exists() {
        return "No stats yet. Run a search first.".to_string();
    }

    let summary = match build_savings_summary(&path) {
        Ok(s) => s,
        Err(e) => return format!("Error reading stats: {e}"),
    };

    let bar_width = 16;
    let heavy_line = format!("  {}", "═".repeat(64));
    let light_line = format!("  {}", "─".repeat(64));

    let mut lines = vec![
        String::new(),
        "  Semble Token Savings".to_string(),
        heavy_line.clone(),
        format!("  {:<12}  {:<6}  Savings", "Period", "Calls"),
        light_line.clone(),
    ];

    for (label, bucket) in &summary.buckets {
        let saved_tokens = bucket.saved_chars / 4;
        let saved_str = if saved_tokens >= 1_000_000 {
            format!("~{:.1}M", saved_tokens as f64 / 1_000_000.0)
        } else if saved_tokens >= 1000 {
            format!("~{:.1}k", saved_tokens as f64 / 1000.0)
        } else {
            format!("~{saved_tokens}")
        };
        let calls_str = if bucket.calls >= 1000 {
            format!("{:.1}k", bucket.calls as f64 / 1000.0)
        } else {
            bucket.calls.to_string()
        };

        if bucket.file_chars > 0 {
            let ratio = bucket.saved_chars as f64 / bucket.file_chars as f64;
            let filled = (ratio * bar_width as f64).round() as usize;
            let bar = format!(
                "{}{}",
                "█".repeat(filled.min(bar_width)),
                "░".repeat(bar_width.saturating_sub(filled))
            );
            let pct = (ratio * 100.0).round() as usize;
            lines.push(format!(
                "  {label:<12}  {calls_str:<6}  [{bar}]  {saved_str} tokens ({pct}%)"
            ));
        } else {
            let bar = "░".repeat(bar_width);
            lines.push(format!(
                "  {label:<12}  {calls_str:<6}  [{bar}]  {saved_str} tokens"
            ));
        }
    }

    if verbose && !summary.call_type_counts.is_empty() {
        lines.push(String::new());
        lines.push("  Usage Breakdown".to_string());
        lines.push(light_line);
        lines.push(format!("  {:<16}  Calls", "Call type"));
        let mut sorted: Vec<_> = summary.call_type_counts.iter().collect();
        sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (call_type, &count) in sorted {
            let count_str = if count >= 1000 {
                format!("{:.1}k", count as f64 / 1000.0)
            } else {
                count.to_string()
            };
            lines.push(format!("  {call_type:<16}  {count_str}"));
        }
        lines.push(heavy_line);
    }

    lines.push(String::new());
    lines.join("\n")
}
