# semble_rs 100-query benchmark

- Binary: `/Users/hunsangjo/.cargo/bin/semble_rs`
- Project: `/Volumes/SAMSUNG/apps/projects/semble`
- Queries: **100**

## Summary

| Metric | Score |
|---|---|
| Recall@1 | **75/100 = 75.0%** |
| Recall@5 | **97/100 = 97.0%** |
| Recall@10 | **100/100 = 100.0%** |
| MRR | **0.8417** |
| Median query time | 163 ms |
| P95 query time | 187 ms |

## Per-category

| Category | n | R@1 | R@5 | R@10 | MRR |
|---|---|---|---|---|---|
| acronym | 10 | 6/10 (60%) | 9/10 (90%) | 10/10 (100%) | 0.710 |
| exact_symbol | 30 | 29/30 (97%) | 30/30 (100%) | 30/30 (100%) | 0.983 |
| korean | 10 | 3/10 (30%) | 8/10 (80%) | 10/10 (100%) | 0.495 |
| nl_feature | 40 | 30/40 (75%) | 40/40 (100%) | 40/40 (100%) | 0.857 |
| scenario | 10 | 7/10 (70%) | 10/10 (100%) | 10/10 (100%) | 0.833 |

## Failures (rank > 5 or not found in top-10)

### Q79 [acronym] `MRR recall metric`

- Expected: `src/stats.rs`
- First-hit rank: **10**
- Top 5 results:
  - `src/outline.rs`
  - `src/digest.rs`
  - `src/search.rs`
  - `src/main.rs`
  - `src/plan.rs`

### Q83 [korean] `검색 결과 점수 필터링`

- Expected: `src/search.rs`
- First-hit rank: **9**
- Top 5 results:
  - `src/digest.rs`
  - `src/outline.rs`
  - `src/tokens.rs`
  - `src/ranking/penalties.rs`
  - `src/filter.rs`

### Q85 [korean] `트리시터 청크 분할`

- Expected: `src/chunking.rs`
- First-hit rank: **7**
- Top 5 results:
  - `src/digest.rs`
  - `src/outline.rs`
  - `src/tokens.rs`
  - `src/filter.rs`
  - `src/plan.rs`
