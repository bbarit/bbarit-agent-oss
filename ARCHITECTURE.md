# Architecture

How bbarit-agent is put together, and the refactoring roadmap we are actively working through. Contributions that move items out of the roadmap are welcome — see [CONTRIBUTING.md](./CONTRIBUTING.md).

## High-level flow

```
main.rs ─▶ lib.rs::run() ─▶ cli.rs (flags) ─▶ config.rs (settings/trust)
                │
                ├─ --upgrade  → update.rs (self-update)
                ├─ --orchestrate → orchestrator.rs (parallel sub-agents)
                ├─ --mode json → lib.rs::run_json_mode
                ├─ --print    → commands.rs::handle_input (one turn)
                └─ default    → tui.rs (interactive)

one turn:  commands.rs::handle_input
             ├─ memory.rs   recall (keyword overlap, cached files)
             ├─ resources.rs skills/prompts (TTL-cached scan)
             ├─ llm.rs      complete() → ProviderCall → provider adapter
             ├─ tools.rs    execute_tool() (read/write/edit/bash/grep/…)
             ├─ mcp.rs      lazily-spawned MCP servers (failure tombstones)
             └─ session.rs  JSONL tree persistence
```

## Module map

| Area | Modules |
| --- | --- |
| Entry / modes | `main`, `lib`, `cli`, `project`, `update` |
| Turn engine | `commands` (agent loop), `llm` (provider adapters), `session` |
| Tools | `tools`, `hashline` (anchored edits), `computer`, `websearch`, `editor` |
| Knowledge | `memory`, `wiki`, `resources` (skills/prompts), `personas` |
| Providers | `providers::{catalog, metadata, registry, costs, types}`, `auth`, `usage` |
| Integrations | `mcp`, `lsp`, `extensions`, `hooks`, `package_cli` |
| UI | `tui`, `stream_ui`, `themes`, `syntax`, `keybindings` |
| Infra | `config`, `trust`, `spawn`, `checkpoints`, `orchestrator`, `bench` |

## Design rules we follow

- **Pattern objectification.** When several functions share the same parameter surface, bundle it into one object. Done: `llm::ProviderCall` carries the request surface for every provider adapter (client, model, thinking, messages, key, config, tools, request config) — adapters destructure it, so call sites and signatures stay uniform.
- **Interop, read-only.** Claude Code (`~/.claude.json`, `~/.claude/skills`)
  and Codex (`~/.codex/config.toml`, `~/.codex/skills`) MCP servers and skills
  are loaded as-is (`mcp::interop_enabled`, gated by `/interop`); bbarit-oss
  never writes to another tool's config.
- **Cache the hot path, invalidate explicitly.** The skills scan is re-read from disk at most once per 5s (`resources::SKILLS_CACHE`), because the system-prompt build asks for it every turn; `/reload` invalidates. MCP servers spawn lazily, stay alive for the process, and failed spawns are tombstoned so one bad `.mcp.json` entry can't re-pay the timeout every turn.
- **Zero clippy warnings.** CI treats fmt as a hard gate; clippy is advisory but the codebase currently carries `0` warnings — keep it that way.
- **Tests are the contract.** 300+ unit tests; user-visible strings that code matches on must update their assertions in the same commit.

## Refactoring roadmap

Ordered by value; each item is sized to land as one reviewable PR.

1. **Split the five mega-modules** into folders with focused submodules, keeping public paths stable via `pub use`:
   - `commands.rs` (\~7k lines) → `commands/{loop,slash,gates,todo}`
   - `tools.rs` (\~6k) → `tools/{spec,fs,search,shell,office,image}`
   - `llm.rs` (\~6k) → `llm/{dispatch,openai,anthropic,google,mistral,bedrock,sse}`
   - `tui.rs` (\~6k) → `tui/{app,render,input,pickers,cards}`
   - `session.rs` (\~2k) → `session/{store,entry,export}`
2. **Objectify the extension runtime invocation.** `extensions::run_node_extension_runtime`takes a mode string plus five per-mode `Option` selectors (13 call sites); fold them into a `RuntimeInvocation` enum so illegal combinations stop compiling.
3. **Message construction.** `session::push_message` (9 args) behind a `MessageDraft` builder used by `push()`/`push_tool()`.
4. **Provider adapter trait.** With `ProviderCall` in place, the natural next step is `trait ProviderAdapter { fn complete(&self, call: ProviderCall) -> Result<Completion> }`and a registry keyed by `ApiKind`, replacing the dispatch `match`.
5. **MCP tool-spec caching.** Tool lists from connected MCP servers are re-requested per spec build; cache per server generation and invalidate on `/reload` (mirrors the skills cache).
6. **Unify duplicated SSE/stream parsing** between the OpenAI-style and Anthropic-style readers in `llm`.

## Invariants to preserve when refactoring

- `--print` keeps stdout answer-only (narration goes to stderr).
- Session JSONL stays readable by older builds (`CURRENT_SESSION_VERSION` gate).
- `.pi`-compatible config discovery keeps working (see PROVENANCE.md).
- Tool names and JSON schemas are a public contract for extensions/MCP.
- `bbarit-oss --upgrade` must never leave a broken binary: write-temp → atomic swap.