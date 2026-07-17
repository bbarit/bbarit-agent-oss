# Changelog

## [0.1.13]

- Reduce per-turn token use by tightening harness role prompts, trimming
  redundant request setup, and keeping app-only tool hints out of the
  standalone OSS agent.
- Restore Codex usage reporting for the current usage payload schema.
- Make cancellation interrupt long-running web tools promptly instead of
  waiting for the network operation to finish.
- Ship smaller release binaries with size optimization, LTO, and stripped
  symbols.
- Harden MCP reload tests against parallel global-state updates and clean up
  DuckDuckGo result-link decoding.

## [0.1.12]

- Windows one-line install: `irm https://bbarit.com/agent/install.ps1 | iex`
  downloads the x64 binary into `%LOCALAPPDATA%\Programs\bbarit-oss` and adds
  it to the user PATH.
- `--upgrade` now works on Windows-on-ARM (Parallels et al.): a natively built
  arm64 binary reported "unsupported platform" because the release matrix has
  no arm64 Windows build — it now upgrades to the x64 binary, which runs under
  Windows emulation.

## [0.1.11]

- Esc now interrupts a codex websocket request that is stuck connecting. The
  dial (TCP connect + TLS/upgrade handshake) had no timeout and ignored
  cancellation, so a stalled server pinned the turn — "cancelling…" for
  minutes. The dial now runs on a helper thread polled against the cancel
  flag, with a 15s connect / 30s handshake-read ceiling of its own.
- The `/resume` session list shows how recently each session was used, e.g.
  "5m ago" / "2h ago" / "3d ago", based on the session file's last write.

## [0.1.10]

- Claude browser OAuth login (`/login anthropic`) is disabled. The flow is kept
  in the codebase but the command and the login picker now point at API keys
  instead, e.g. `/login anthropic sk-ant-api03-...`. Existing OAuth logins and
  API-key logins keep working.

## [0.1.9]

- `/model` now always saves the picked model as the launch default. It used to
  skip the save when the provider had no API key yet (the login prompt opens
  right after), so the choice silently reverted on the next launch — "model
  switching doesn't save".
- Esc now interrupts MCP server startup handshakes. A slow or broken MCP entry
  (including ones inherited from Claude Code's `~/.claude.json` via interop)
  could pin the start of a turn for its full timeout while ignoring
  cancellation. A cancelled handshake is not marked failed, so the server
  retries normally on the next turn.

## [0.1.6]

- Esc now cancels promptly even while the model is "thinking" and streaming
  nothing (opus:high, o-series, gemini thinking). The blocking SSE read is served
  by a reader thread so cancellation is checked ~5×/s instead of waiting for the
  model to reply — fixes "Esc does nothing" during long thinking pauses.

## [0.1.5]

- Fix the "update available" startup hint: the background version check usually
  landed after the splash was built, so the hint never showed — it now appears on
  the first frame after the check completes.

## [0.1.4]

- Sign-in on demand: when a turn fails because the active provider has no API
  key, bbarit-oss now opens the login picker (and names the provider) instead of
  showing a dead-end error — one keystroke from signing in.

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
