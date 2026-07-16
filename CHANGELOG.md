# Changelog

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
- Document memory, wiki, and personas in detail; add ARCHITECTURE.md.

## [0.1.0]

- First open-source release of the standalone bbarit coding agent.
- Terminal-only build: multi-provider LLM support, agent loop, tools
  (read/write/edit/bash/grep/find/ls), TUI, sessions, skills, extensions, LSP,
  and MCP.
- Removed host-app integrations (embedded browser, media generation, app
  control, RPC embed mode) so the agent runs fully standalone.
