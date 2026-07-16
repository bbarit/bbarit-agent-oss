<!-- Keywords: 코드 검색, 시맨틱 코드 검색, AI 에이전트, LLM, BM25, 임베딩, tree-sitter, AST, 의존성 그래프, 영향 분석, Rust, CLI, Claude Code, Codex, Cursor, grep 대체, 토큰 절감, potion-code, model2vec, 하이브리드 검색, RRF, 빌드 출력 압축, CI 로그 압축, 한글 코드 검색, korean code search -->

<h2 align="center"> semble_rs<br/> 에이전트를 위한 빠르고 정확한 코드 검색 — Rust로<br/> <sub>grep / cat / read / ls 대체 + 빌드 / CI 출력 압축. 최대 <b>-99%</b> 토큰.</sub> </h2>

<div align="center">

<p>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.75%2B-orange.svg" alt="Rust"></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg" alt="Platform">
  <a href="#%EB%B2%A4%EC%B9%98%EB%A7%88%ED%81%AC"><img src="https://img.shields.io/badge/agent%20tokens-up%20to%20--99%25-brightgreen.svg" alt="Token savings"></a>
  <a href="./README.md"><img src="https://img.shields.io/badge/English-README.md-blue.svg" alt="English"></a>
</p>

<p>
  <a href="#%EB%B9%A0%EB%A5%B8-%EC%8B%9C%EC%9E%91">빠른 시작</a> •
  <a href="#search">Search</a> •
  <a href="#tree">Tree</a> •
  <a href="#digest">Digest</a> •
  <a href="#%EC%9D%98%EC%A1%B4%EC%84%B1-%EA%B7%B8%EB%9E%98%ED%94%84">Deps / Impact</a> •
  <a href="#%EA%B5%AC%EC%A1%B0">구조</a> •
  <a href="#%EB%B2%A4%EC%B9%98%EB%A7%88%ED%81%AC">벤치마크</a>
</p>

</div>

`semble_rs`는 [MinishLab/semble](https://github.com/MinishLab/semble)의 Rust 포팅 + 확장판으로, AI 코딩 에이전트를 위해 설계되었습니다. 에이전트가 필요로 하는 정확한 코드 청크만 반환하고, `ls -R` 대신 토큰이 저렴한 코드베이스 트리를 출력하며, 3 MB CI 로그를 35 KB로 압축합니다. 단일 Rust 바이너리, daemon 없음, API key 없음, GPU 불필요. BM25 + [Model2Vec](https://github.com/MinishLab/model2vec) 정적 임베딩의 하이브리드 검색에 코드 인식 reranking이 더해지고, 의존성 그래프와 AST 청킹, 빌드 / 테스트 / CI 출력을 위한 `digest` 파이프라인을 함께 제공합니다.

## 빠른 시작

```bash
# Rust 설치 후:
git clone https://github.com/johunsang/semble_rs.git && cd semble_rs
cargo install --path .
```

바이너리는 `~/.cargo/bin/semble_rs`에 설치됩니다. 첫 실행 시 기본 임베딩 모델 `minishlab/potion-code-16M` (\~60 MB)이 HuggingFace에서 자동 다운로드됩니다.

```bash
# 코드베이스 지도 (ls -R 대체)
semble_rs tree ./my-project --symbols

# 의미로 코드 찾기 (grep + cat 대체)
semble_rs search "인증 어디서 처리하지" ./my-project --outline

# 빌드 / CI 출력 압축
cargo build 2>&1 | semble_rs digest
gh run view <id> --log-failed | semble_rs digest
```

에이전트(Claude Code, Codex, Cursor) 통합은 [에이전트 통합](#%EC%97%90%EC%9D%B4%EC%A0%84%ED%8A%B8-%ED%86%B5%ED%95%A9) 참조.

## 주요 기능

- **빠름**: 이 저장소(22 파일) 인덱싱 \~150 ms, 1,600 파일 \~10 s. 정적 임베더 — 쿼리 시 transformer forward pass 없음.
- **토큰 효율**: `tree`는 `ls -R` 대비 **4×–747×** 압축, `--outline`은 `--compact` 대비 **-47%**, `digest`는 실제 GitHub Actions 로그에서 **-98.9%**.
- **하이브리드 검색**: BM25 + Model2Vec 임베딩을 RRF로 융합 후 정의 / 식별자 stem / 파일 일관성 boost와 노이즈 페널티로 reranking.
- **의존성 그래프**: `deps` / `impact`가 파일이 import하고 정의하는 것 + 변경 시 영향받는 파일을 표시. Graphviz `--dot` 출력 옵션.
- **빌드 / CI 압축**: `digest`가 cargo, pnpm/npm/yarn/bun, tsc, pytest, go test, gradle, ruff, mypy, clang/gcc/cmake/make/swiftc, GitHub Actions를 자동 감지.
- **단일 바이너리**: Python 없음, daemon 없음, API key 없음. CPU에서 동작.

## Search

```bash
semble_rs search "인증 흐름" ./my-project --outline    # 1단계: 구조 스캔
semble_rs search "loginWithEmail" ./my-project --compact   # 2단계: 매칭 라인
semble_rs search "save model" https://github.com/MinishLab/model2vec   # git URL
```

`path`는 생략 시 현재 디렉토리. git URL도 받음 (shallow clone).

### 출력 모드

| 모드 | 출력 | `--compact` 대비 토큰 | 용도 |
| --- | --- | --- | --- |
| `--outline` | 청크당 시그니처 1줄 | **-47%** | 1단계 구조 스캔 |
| `--group` | 디렉토리 그룹 + 매칭 라인 최대 3개 (`+N`로 overflow) | \-47% | 청크당 매칭이 많을 때 |
| `--compact` | 점수 + 경로 + 모든 매칭 라인 | baseline | 정밀 스캔 |
| `--json --strip` | 청크 본문 (주석 제거) | +800% | 도구/파이프라인 연동 |
| `--json` | 청크 본문 (raw) | +900% | 도구/파이프라인 연동 |

**권장 워크플로:** `--outline`로 구조 파악 → `--compact`로 좁히기 → 청크 본문 자체가 필요할 때만 `--json --strip`.

### `find-related`

이전 검색 결과의 `file:line`이 주어지면, 그 위치와 의미적으로 유사한 청크를 반환합니다.

```bash
semble_rs find-related src/auth.rs 42 ./my-project
```

### `plan`

에이전트가 어디서부터 시작할지 모를 때, `plan`은 작은 검색을 돌리고 추천 명령 시퀀스(`--outline` / `--group` / `--compact` / `deps` / `impact`)를 출력합니다.

```bash
semble_rs plan "인증 버그 고치기" ./my-project -k 5
```

`plan`은 가이드일 뿐 정답이 아닙니다. 신뢰도 낮은 후보는 단서이지 사실이 아니므로 자연어 쿼리를 넓혀 재검색하세요. 심볼/기능명을 이미 알면 건너뜁니다.

### `--model`

모든 search 계열 명령은 `--model <hf-repo-or-local-path>` 옵션으로 임베더를 교체할 수 있습니다. `SEMBLE_MODEL_PATH` 환경변수도 지원.

## Tree

`semble_rs tree`는 `search`와 동일한 gitignore 인지 인덱스를 사용해 코드베이스 파일 트리를 출력합니다. `ls -R`은 실제 프로젝트에서 `.git/`, `target/`, `node_modules/`까지 모두 포함해 토큰 수만에서 수십만으로 폭발하기 때문입니다. 실측:

| 프로젝트 | `semble_rs tree` | `ls -R` | 압축률 |
| --- | --- | --- | --- |
| 이 저장소 (Rust + `target/`) | **533 B** | 398,101 B | **747×** |
| 6,693 파일 Python 백엔드 | **3,950 B** | 254,066 B | **64×** |
| 325 파일 ML 학습 저장소 | 838 B | 7,522 B | 9× |

```bash
semble_rs tree                              # 현재 디렉토리
semble_rs tree -d                           # 디렉토리만
semble_rs tree --max-depth 2                # 깊이 제한
semble_rs tree --symbols                    # 파일별 top-level 심볼 동봉
semble_rs tree --lang rust,python           # 언어 필터
```

## Digest

`semble_rs digest`는 빌드 / 테스트 / 설치 / CI 출력을 압축합니다. 에러, `file:line:col`, traceback, panic stack, 실패 step 본문은 항상 보존되고 진행 라인만 카운트로 축약됩니다.

```bash
cargo build 2>&1            | semble_rs digest
pnpm install 2>&1           | semble_rs digest
pytest 2>&1                 | semble_rs digest
gh run view <id> --log-failed | semble_rs digest
```

15개 실측 fixture:

| Fixture | Raw → digest | 절감 |
| --- | --- | --- |
| `cargo build` (clean, 218 crates) | 7,611 B → 59 B | **-99.2%** |
| `cargo test` (45 통과) | 3,368 B → 369 B | \-89.0% |
| `pnpm install` | 1,323 B → 349 B | \-73.6% |
| `tsc` (13 errors, 5 codes) | 1,085 B → 648 B | \-40.3% |
| `pytest` (4 failures) | 2,762 B → 2,330 B | \-15.6% |
| **GitHub Actions log (rust-lang/rust 실패 CI, real)** | **3.3 MB → 35 KB** | **-98.9%** ⭐ |
| `go test` (panic + stack 포함) | 1,034 B → 475 B | \-54.1% |
| `gradle test` (2 failures) | 1,232 B → 522 B | \-57.6% |
| `ruff` / `mypy` / `clang` / `cmake` / `swift` | 가변 | \-3% \~ -30% |
| **TOTAL (15 fixtures)** | **3.33 MB → 43 KB** | **-98.7%** |

자동 감지: cargo, pnpm/npm/yarn/bun, tsc, pytest, go test, gradle, ruff, mypy, clang/gcc/cmake/make/swiftc, GitHub Actions. `--format <name>`으로 강제 지정, `--show-format`으로 감지된 핸들러 확인.

## 의존성 그래프

```bash
semble_rs deps   src/auth.rs ./my-project                  # 이 파일이 import / 정의하는 것 (flat)
semble_rs deps   src/auth.rs ./my-project --tree           # transitive import를 ASCII 트리로
semble_rs deps   src/auth.rs ./my-project --tree --max-depth 3
semble_rs deps   src/auth.rs ./my-project --dot | dot -Tpng > deps.png
semble_rs impact src/auth.rs ./my-project                  # 누가 이 파일을 의존하나 (flat)
semble_rs impact src/auth.rs ./my-project --tree           # 역방향 의존성 트리
semble_rs impact src/auth.rs ./my-project --dot | dot -Tpng > impact.png
```

`--tree` (v0.9.1+) 옵션은 forward (`deps`) 또는 reverse (`impact`) 의존성을 ASCII 트리로 렌더링합니다. 사이클은 `(cycle)`로 표시되고, `--max-depth N`로 깊이 제한 시 `…`로 잘림을 표시. 외부 도구 불필요, 에이전트 친화적.

`impact`는 공유 모듈을 수정하기 전에 영향 범위를 먼저 파악하기 위한 명령입니다.

### `find-pattern`

시맨틱 검색이 표현 못 하는 구조적 쿼리를 위한 `ast-grep` 얇은 wrapper:

```bash
semble_rs find-pattern 'fn $name($$$)' . --lang rust --compact
```

`ast-grep` 설치 필요 (`brew install ast-grep` 또는 `cargo install ast-grep`).

## Encode

`semble_rs encode`는 임베딩 모델을 CLI로 노출합니다 (스크립팅 / 디버깅용):

```bash
semble_rs encode "검색 결과 점수"            # 단일 벡터 → JSON 배열
echo -e "auth\nlogin\ntoken" | semble_rs encode     # stdin, 한 줄당 한 sentence
semble_rs encode "x" --model minishlab/potion-multilingual-128M
```

## 에이전트 통합

프로젝트 루트의 `CLAUDE.md` 또는 `AGENTS.md`에 다음 같은 스니펫을 붙이면 Claude Code, Codex, Cursor (`.cursorrules`), Aider, OpenHands에서 모두 작동합니다.

```markdown
## 코드 검색과 탐색

`ls -R`, `grep`, `cat` 대신 `semble_rs`를 사용:

​```bash
semble_rs tree . --symbols                         # 코드베이스 지도 (저렴)
semble_rs search "<feature or symbol>" . --outline # 1단계
semble_rs search "<feature or symbol>" . --compact # 2단계
semble_rs deps   <file> .                          # 파일이 import / 정의하는 것
semble_rs impact <file> .                          # 변경 영향받는 파일
​```

노이즈가 큰 명령 출력은 읽기 전에 압축:

​```bash
cargo build 2>&1 | semble_rs digest
pnpm install 2>&1 | semble_rs digest
gh run view <id> --log-failed | semble_rs digest
​```
```

`semble_rs savings`로 누적 토큰 절감량을 확인할 수 있습니다.

## 구조

`semble_rs`는 `tree-sitter`로 모든 파일을 함수 / 클래스 / 모듈 단위 청크로 분할합니다 (지원 안 되는 언어는 라인 기반 fallback). 그 후 두 개의 보완적 retriever로 쿼리를 채점합니다: 시맨틱 유사도용 정적 [Model2Vec](https://github.com/MinishLab/model2vec) 임베딩 (기본 `minishlab/potion-code-16M`), 그리고 식별자 / API명 매칭용 BM25. 두 점수 리스트는 Reciprocal Rank Fusion으로 융합됩니다.

융합 후 코드 인식 신호로 reranking:

<details> <summary><b>Ranking 신호</b></summary>

- **Adaptive weighting.** 심볼형 쿼리 (`Foo::bar`, `_private`, `getUserById`)는 lexical 가중치 ↑, 자연어 쿼리는 시맨틱/lexical 균형 유지.
- **Definition boosts.** 쿼리한 심볼을 정의하는 청크 (`class`, `def`, `func` 등)가 단순 참조 청크보다 위로.
- **Identifier stems.** 쿼리 토큰을 stemming 후 식별자 stem과 매칭. `parse config` 검색 시 `parseConfig`, `ConfigParser`, `config_parser` 부스트.
- **File coherence.** 같은 파일에서 여러 청크가 매칭되면 파일 자체에 부스트 → top 결과가 파일 단위 적절성을 반영.
- **Sibling-chunk boost.** top hit과 인접한 청크에 작은 부스트 — 정의와 helper는 보통 클러스터.
- **Dependency boost.** top hit이 import하는 파일의 청크에 부스트 → call-chain 컨텍스트가 떠오름.
- **노이즈 페널티.** 테스트 파일, `compat/` / `legacy/` shim, example 코드, `.d.ts` 선언 stub은 down-rank → canonical 구현이 먼저.

</details>

임베더는 완전 정적 (vocab 임베딩 lookup → mean pool → SIF weighting → L2 정규화). 모든 동작이 CPU에서 ms 단위로 완료.

## 벤치마크

### 검색 품질 — 100-query 벤치마크 (이 저장소)

수동 라벨링된 100개 쿼리, 5개 카테고리: 정확 심볼명, 자연어 기능 설명, 시나리오, 약어, 한국어. 기본 모델 `minishlab/potion-code-16M`.

| Metric | Score |
| --- | --- |
| Recall@1 | 70% |
| Recall@5 | 90% |
| Recall@10 | 95% |
| MRR | 0.78 |
| Median latency | 150 ms / query (cold) |

| Category | n | R@1 | R@5 | R@10 | MRR |
| --- | --- | --- | --- | --- | --- |
| exact_symbol | 30 | 93% | 100% | 100% | 0.96 |
| nl_feature | 40 | 75% | 98% | 100% | 0.83 |
| scenario | 10 | 70% | 100% | 100% | 0.77 |
| acronym | 10 | 50% | 70% | 70% | 0.56 |
| korean | 10 | 10% | 60% | 80% | 0.27 |

Query 셋: `docs/eval_set_100.json` · 미스 분석: `docs/benchmark_100.md`.

### 인덱싱과 쿼리 latency (저장소 크기별)

인덱스는 매 호출마다 재구축 (persistent cache 없음).

| 저장소 크기 (코드 파일 수) | 인덱싱 + 첫 쿼리 |
| --- | --- |
| 22 (이 저장소) | **\~0.15 s** |
| 57–120 | \~0.3–0.7 s |
| 1,600 | \~10 s |

`digest`는 저장소 크기와 무관: 3.3 MB CI log → 35 KB in **\~20 ms**.

### 토큰 효율 (네이티브 shell 도구 대비)

실측:

| 작업 | `semble_rs` | 네이티브 | 절감 |
| --- | --- | --- | --- |
| **코드베이스 지도** (이 저장소) | `tree` **533 B** | `ls -R` 398 KB | **747×** |
| **코드베이스 지도** (6,693 파일 Python 백엔드) | `tree` **3,950 B** | `ls -R` 254 KB | **64×** |
| **코드베이스 지도** (325 파일 Python 저장소) | `tree` 838 B | `ls -R` 7,522 B | 9× |
| **코드 청크 조회** (`--outline` vs `--compact`) | \-47% | baseline | \-47% |
| **빌드 로그** (`cargo build` clean) | `digest` 59 B | raw 7,611 B | **-99.2%** |
| **CI 실패 로그** (실제 GitHub Actions, rust-lang/rust) | `digest` 35 KB | raw 3.3 MB | **-98.9%** ⭐ |
| **15 fixture 합산** | `digest` 43 KB | raw 3.33 MB | **-98.7%** |

> `grep + cat + ls -R`을 쓰는 에이전트는 컨텍스트 윈도우 대부분을 무관 코드와 노이즈에 소모합니다. `semble_rs`는 필요한 것만 반환하고 나머지는 압축합니다.

## 지원 언어

| 언어 | 검색 | AST 청킹 | 의존성 그래프 |
| --- | --- | --- | --- |
| Rust | ✓ | ✓ | ✓ |
| Python | ✓ | ✓ | ✓ |
| JavaScript / TypeScript | ✓ | ✓ | ✓ |
| Go | ✓ | ✓ | ✓ |
| Java | ✓ | ✓ | ✓ |
| C / C++ | ✓ | ✓ | ✓ |
| Kotlin | ✓ | ✓ | ✓ |
| Ruby | ✓ | ✓ | ✓ |
| PHP | ✓ | ✓ | ✓ |
| Swift | ✓ | ✓ | ✓ |
| HTML / CSS / Vue / Svelte | ✓ | line-based | partial |
| 그 외 | ✓ | line-based | — |

## License

MIT

## Acknowledgements

- [MinishLab/semble](https://github.com/MinishLab/semble) — Stéphan Tulkens, Thomas van Dongen의 원조 Python 구현. `semble_rs`는 그 작업의 Rust 포팅 + 확장판입니다.
- [Model2Vec](https://github.com/MinishLab/model2vec)와 [model2vec-rs](https://github.com/MinishLab/model2vec-rs) — 임베더의 기반 정적 distillation 프레임워크.
- 임베딩 모델: `minishlab/potion-code-16M`.