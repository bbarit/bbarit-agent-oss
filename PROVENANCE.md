# Provenance & Source Similarity

**Short version:** `bbarit-agent` is an independent Rust implementation whose
architecture and behavior are **inspired by and based on [Pi](https://github.com/earendil-works/pi)**
(MIT, © 2025 Mario Zechner). The Rust code was written from scratch — but because
both talk to the same LLM providers and Pi's design was the reference, some
**data and interoperability constants are necessarily identical**. In addition,
the **automatic-memory system is modeled on [qwen-code](https://github.com/QwenLM/qwen-code)**
(Apache-2.0). This document discloses exactly what overlaps, and by how much,
measured directly.

We publish these numbers so nobody has to guess. Everything below is
reproducible from the sources.

## Lineage

bbarit-agent was originally developed as the agent **embedded in BBARIT Terminal**
(our desktop AI coding IDE) and is now extracted and published as a standalone
open-source CLI.

```
Pi  (earendil-works/pi, MIT © 2025 Mario Zechner)     ← agent core & design
 └─ bbarit-agent  ← this project: a Rust implementation based on Pi's design,
                    originally shipped inside BBARIT Terminal

qwen-code  (QwenLM/qwen-code, Apache-2.0)             ← auto-memory system design
```

The agent core is based on Pi; the automatic-memory subsystem
(`src/memory.rs`) is modeled on qwen-code's auto-memory design (see
[Adapted from qwen-code](#adapted-from-qwen-code) below).

## What was reused vs. written from scratch

| Reused from Pi (not original) | Written from scratch for bbarit-agent |
|---|---|
| High-level architecture & behavior (config layout, session format, provider-registry design, agent loop shape) | All Rust source code |
| Model **catalog data** (provider IDs, model IDs, display names, context windows) | Every module's implementation |
| **API endpoints** and **OAuth flows/scopes** (must match the real services) | 99.9% of code comments (independently authored) |
| Protocol constants (e.g. LSP method names) | The TUI, tool implementations, and CLI surface |

Architecture, behavior, public API shape, and factual data (model IDs, endpoints,
protocol names) are **not copyrightable** — this is the settled rule from
*Google LLC v. Oracle America, Inc.* An implementation that reproduces
another program's behavior in a different language, without copying its
expressive text, is a clean reimplementation.

## How bbarit-agent differs from Pi

bbarit-agent took Pi's design as a starting point, but it is **not a mirror of
Pi**. The differences are substantial — a different language, a different scope,
and a set of features Pi does not have at all.

**Philosophy: keep Pi's simplicity, then build further.** What we deliberately
kept is Pi's minimalist core — a small, legible agent loop and a tight set of
first-class tools. What we changed is everything around it: we reimplemented that
core in Rust and then advanced the parts that matter most for day-to-day work —
a stronger **multi-process orchestrator** for parallel sub-agents, a built-in
**project wiki**, a **295-persona** system, and bundled **semantic code search**.
We also added an automatic-**memory** system, whose design is adapted from
qwen-code (credited below). The result keeps Pi's "small and understandable" feel
while going well beyond it in capability.

### 1. A complete rewrite in a different language

Pi is ~206,000 lines of **TypeScript** across five packages and requires a
Node.js runtime. bbarit-agent is ~53,000 lines of **Rust** in a single crate
that compiles to **one self-contained static binary** — no interpreter, no
`node_modules`, no runtime install. None of Pi's source lines exist in
bbarit-agent; every line was written from scratch. (This is why the code-comment
overlap measured below is 0.1% and creative-text overlap is 0.)

### 2. Features original to bbarit-agent (absent from Pi)

These were built by the bbarit-agent maintainers on top of the ported core — they
are **our own additions**, not present in Pi. Each was verified to have **zero
presence** in the Pi source:

| Feature | What it is |
|---|---|
| **Persona system** | 295 curated specialist personas across 32 domains, selectable with `--persona` / `/persona`, injected at startup. |
| **Built-in wiki** | A per-project knowledge base (`wiki.rs`). |
| **Bundled semantic code search** | Vendors the `semble` engine for a `code_search` tool and a background-warmed code index — hybrid BM25 + semantic, over the project. |
| **Computer-use tool** | Opt-in screenshot + mouse/keyboard control (`computer.rs`). |
| **Hash-addressed editing** | `hashline`-based edit application for robust patching. |

### 3. Narrower, sharper scope

Rather than reproduce all five Pi packages, bbarit-agent reimplements a curated
subset of the agent core and deliberately **drops** what it does not need. This
open-source build is terminal-only: host-app integrations (embedded browser,
media generation, desktop app control, and the RPC embed protocol) have been
removed entirely.

### 4. Data-compatible but independently maintained

The model catalog (1,057 models), provider endpoints, and OAuth flows are kept
**interoperable** with the wider ecosystem — that is the source of most of the
verbatim overlap measured below — but the catalog, defaults, and provider wiring
are maintained independently in this repository.

The net effect: the two programs share a **family resemblance in behavior**, and
the factual constants any LLM agent must use, while the implementation, the
feature set, and a large amount of the surface are bbarit-agent's own.

## Measured similarity (verbatim overlap)

**Method (reproducible):** extract every string literal (≥ 30 chars) and every
`//` comment (≥ 25 chars) from both source trees, normalize whitespace, and
compute the exact (character-for-character) set intersection. Compared against
Pi `v0.80.2` (commit `622eca7`).

| Metric | Overlap with Pi |
|---|---|
| Distinct string literals (≥30 chars) | **254 / 1,983 = 12.8%** by count · **4.0%** by character weight |
| Code comments (≥25 chars) | **5 / 3,536 = 0.1%** |
| Distinctive **creative prose** (original expression) | **0** |

### What the 254 string matches actually are

Every one of the 254 shared strings is factual or interoperability data — not
creative expression:

- **~157** — model IDs, config keys, and constants (e.g. `global.anthropic.claude-sonnet-4-5-20250929-v1:0`)
- **~59** — model **display names** from the providers (e.g. `Anthropic: Claude Opus 4.8 (Fast)`, `Google: Gemini 2.5 Pro Preview 06-05`)
- **~30** — API endpoint URLs (e.g. `https://api.anthropic.com`, `https://generativelanguage.googleapis.com/v1beta`)
- **2** — OAuth scope strings (e.g. `openid profile email offline_access` — fixed by the provider)
- remaining — short interop tokens

These are values you **cannot change** without breaking the software: you have to
POST to the real Anthropic URL, request the real model ID, and send the exact
OAuth scope. Any independent multi-provider agent ends up with the same set.

The **5 comment matches** are likewise functional — they are auth endpoint URLs
that happen to appear inside comments (e.g. `auth.openai.com/api/accounts/deviceauth/token`).

**No distinctive prose from Pi remains** in bbarit-agent. Where Pi's wording was
originally carried over (a handful of system-prompt lines and error messages),
those strings have been rewritten in bbarit-agent's own words.

## Adapted from qwen-code

The automatic-memory system (`src/memory.rs`) is modeled on the auto-memory
design of [qwen-code](https://github.com/QwenLM/qwen-code) (Apache-2.0): the
durable-fact taxonomy (`user` / `feedback` / `project` / `reference`) and the
`MEMORY.md` index concept come from there. The Rust implementation and its
prompts were written independently. qwen-code's attribution is recorded in
[`NOTICE`](./NOTICE).

## License compliance

Pi is MIT-licensed, which permits reuse, modification, and redistribution — the
only condition is that Pi's copyright and permission notice be preserved. That
notice is included verbatim in [`NOTICE`](./NOTICE). bbarit-agent is released
under the MIT license (see [`LICENSE`](./LICENSE)).

## Reproduce these numbers

1. Clone Pi at `v0.80.2`: `git clone https://github.com/earendil-works/pi && git -C pi checkout 622eca7`
2. Extract string literals (`"…"`) and `//` comments from `pi/packages/**/*.ts`
   and from `bbarit-agent/src/**/*.rs`.
3. Normalize whitespace, keep entries ≥ 30 chars (strings) / ≥ 25 chars (comments),
   and intersect the two sets.

The analysis scripts used to produce this report are available on request.
