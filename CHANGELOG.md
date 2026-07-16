# Changelog

## [0.1.3]

- First-run onboarding: a fresh install with no credentials opens the login
  picker on launch (plus a one-line welcome) instead of failing on the first
  message — pick a provider and sign in, or press Esc and run `/login` anytime.
- Harden the Gemini tool-schema sanitizer: a bare `{"type":"array"}` now gets a
  default `items`, so loosely-typed array params no longer 400.

## [0.1.2]

- Fix Gemini 3.x tool calls: preserve and echo back each functionCall's
  `thoughtSignature`, which the API now requires — multi-turn tool use
  previously failed with HTTP 400 on the default Gemini model.
- Reuse Claude Code (`~/.claude.json`, `~/.claude/skills`) and Codex
  (`~/.codex/config.toml`, `~/.codex/skills`) MCP servers and skills as-is —
  on by default, toggle with `/interop` or `BBARIT_INTEROP=0`.
- Register an MCP server or scaffold a skill in one keystroke: `/mcp add`,
  `/mcp remove`, `/skill new`.
- Startup update check: a non-blocking background check shows an "update
  available" hint; `/update` applies it, `BBARIT_AUTO_UPGRADE=1` upgrades in
  place at launch, `BBARIT_NO_UPDATE_CHECK=1` disables it.
- Note management: `/wiki get|delete|reset` and `/memory show|reset` to load,
  remove, and clear notes and memories.

## [0.1.1]

- Show version and platform at startup (splash and title bar) and refresh the
  splash to the BBARIT AGENT OSS wordmark.
- `--upgrade` now refuses to downgrade when the server manifest is older, and
  never leaves a temp file behind on failure.
- Atomic installs from install.sh (same-filesystem rename) and a release
  pipeline that fails loudly if the update channel does not deploy.
- Cache the skills scan (5s TTL) so the per-turn system-prompt build stops
  re-reading disk; `/reload` invalidates.
- Zero clippy warnings; bundle the provider-adapter request surface into one
  `ProviderCall` object; remove dead references and menus left over from
  host-app-only integrations.
- Fully separate from BBARIT Terminal: own `~/.bbarit-oss` home, own note
  vault and dotenv, no editor/office/app bridges. Binary renamed `bbarit-oss`.
- Sanitize tool schemas for Gemini (strip `additionalProperties`/`anyOf`),
  fixing HTTP 400 on Google providers.
- Document memory, wiki, and personas in detail; add ARCHITECTURE.md.

## [0.1.0]

- First open-source release of the standalone bbarit coding agent.
- Terminal-only build: multi-provider LLM support, agent loop, tools
  (read/write/edit/bash/grep/find/ls), TUI, sessions, skills, extensions, LSP,
  and MCP.
- Removed host-app integrations (embedded browser, media generation, app
  control, RPC embed mode) so the agent runs fully standalone.
