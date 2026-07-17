# bbarit-oss — driving it from a program

The official interface for when a **program** (Claude Code, Codex, a script) —
not a human — drives bbarit-oss. This one document is all you need.

## Build / location

```bash
cargo build --release          # → target/release/bbarit-oss
```

When piping to it from a program, always set `BBARIT_AGENT_MODE=1`.

## 1) One-shot — `--print` (simplest)

```bash
BBARIT_AGENT_MODE=1 bbarit-oss --print --no-pick --no-session \
  --provider anthropic --model claude-sonnet-5 \
  "Answer in one line: how is this repo built?"
```

**stdout/stderr contract (when piped):**

- **stdout** — only the final assistant answer, as one block. Narration,
  thinking, and `⚙ tool` activity lines are never mixed in.
- **stderr** — the live token stream and activity lines (for progress; do not parse).

So a consumer can just take all of stdout after `2>/dev/null` as the answer.

Key flags: `--no-pick` (skip the folder picker) · `--no-session` (leave no
session file) · `--no-tools` · `-t bash,read,edit` (tool allowlist) ·
`--thinking low|medium|high` · `--persona <id>` · `--append-system-prompt "..."`.

## 2) Structured events — `--mode json`

Runs one turn and streams JSON Lines to stdout:

```
{"type":"session","id":...,"cwd":...}
{"type":"agent_start"} {"type":"turn_start"}
{"type":"message_update","delta":{"type":"text_delta","text":"..."}}   # with --stream
{"type":"message_end","message":{...}}                                  # user/assistant/tool each
{"type":"turn_end","message":<final message>,"toolResults":[...]}
{"type":"agent_end","messages":[full history]}
```

```bash
BBARIT_AGENT_MODE=1 bbarit-oss --mode json --no-pick --no-session "task..."
```

## 3) Parallel sub-agents — `--orchestrate`

```bash
BBARIT_AGENT_MODE=1 bbarit-oss --orchestrate "task A" "task B" "task C"
```

Runs each positional input as an independent sub-agent process in parallel and
collects the results.

## 4) Harness — plan → develop → review with per-role models

`/harness` runs one task through a role-separated pipeline (planner →
developer ⇄ reviewer). Each role can use its own model:

```bash
# Persist role models (planner developer reviewer, in order), save as a preset:
bbarit-oss -p '/roles zai/glm-5.2 openai/gpt-5.2 anthropic/claude-opus-4-8'
bbarit-oss -p '/roles save myteam'

# Run with the preset — or override models for one run only (nothing saved):
bbarit-oss --approve -p '/harness @myteam add input validation'
bbarit-oss --approve -p '/harness @planner=zai/glm-5.2,reviewer=openai/gpt-5.2 fix the bug'
bbarit-oss --approve -p '/harness @m1 @m2 @m3 <task>'   # positional: planner dev reviewer
```

Other forms: `/roles planner=<m> developer=<m> reviewer=<m>` ·
`/roles <model>` (all roles) · `/roles presets` · `/roles delete <name>` ·
`/roles <role> persona <id>`.

## Environment variables

| Variable | Meaning |
| --- | --- |
| `BBARIT_AGENT_MODE=1` | Agent mode (required when run standalone from a program) |
| `BBARIT_SUBAGENT=1` | Mark as a sub-agent — blocks recursive behavior such as auto-memory extraction |
| `BBARIT_AUTO_CONTEXT=0` | Disable start-of-turn auto-RAG (code-context injection) — faster for non-code tasks |
| `BBARIT_AUTO_MEMORY=0` | Disable auto-memory recall/extract |
| `BBARIT_PERSONA=<id>` | Set the startup persona |
| `BBARIT_INTEROP=1` | Reuse Claude Code / Codex MCP servers & skills (default: off) |
| `BBARIT_AUTO_UPGRADE=1` | Upgrade in place at startup when a newer release exists (default: off) |
| `BBARIT_NO_UPDATE_CHECK=1` | Disable the background "update available" check at startup |
| `BBARIT_CONTEXT_FILE_LIMIT=<bytes>` | Per-file cap for AGENTS/CLAUDE.md prompt injection (default 20000; 0 = unlimited) |

## Performance notes

- The code-context index (semble) is a **process-global cache**. It is pre-warmed
  in the background at startup and rebuilt in the background with a 60s debounce
  after file-mutating tools run. It never blocks a turn — the first turn goes out
  without context if the index isn't ready yet.
- A one-shot `--print` process usually finishes before the index is ready, so
  when code search is needed the model calls the `code_search` tool (the only
  time a blocking build happens).

## Latency smoke test

```bash
time (BBARIT_AGENT_MODE=1 target/debug/bbarit --print --no-pick --no-session \
  "Reply with exactly: pong" 2>/dev/null)
# Expect: stdout == "pong", wall clock ≈ one LLM round trip (~3s), CPU < 2s
```
