# Release & build process

Two goals, both automated:

- **Users update easily** — `curl … | sh` to install, `bbarit-oss --upgrade` to update.
- **We build & ship easily** — push one tag; GitHub Actions builds every platform
  and publishes.

```
bump version ─▶ git tag vX.Y.Z ─▶ push
                     │
                     ▼
        GitHub Actions (.github/workflows/release.yml)
   build macOS arm64/x64 · Linux x64/arm64 · Windows x64
                     │
        ┌────────────┴────────────┐
        ▼                         ▼
  GitHub Release           deploy to bbarit.com
  (binaries + latest.json)  (dist/<ver>/ + latest.json + install.sh)
                     │
                     ▼
   users:  curl … | sh   ·   bbarit-oss --upgrade
```

## Cut a release

```sh
# 1. Bump the version in Cargo.toml (and CHANGELOG.md)
#    version = "0.1.1"

# 2. Commit, tag, push
git add -A && git commit -m "v0.1.1"
git tag v0.1.1
git push origin main --tags
```

Pushing the `v*` tag triggers **`.github/workflows/release.yml`**, which:

1. Builds a release binary for all five targets on GitHub-hosted runners.
2. Generates `latest.json` (version + per-platform download URLs).
3. Publishes a **GitHub Release** with every binary + `latest.json`.
4. If deploy secrets are set, uploads the binaries, `latest.json`,
   `install.sh`, and `install.ps1` to **bbarit.com** so the installers and
   `--upgrade` resolve.

## One-time GitHub setup

For the auto-deploy-to-bbarit.com step, add these repository **secrets**
(Settings → Secrets and variables → Actions):

| Secret | Value |
|---|---|
| `BBARIT_SSH_KEY` | private SSH key with access to the server |
| `BBARIT_SSH_HOST` | server host (e.g. `bbarit.com`) |
| `BBARIT_SSH_USER` | SSH user (e.g. `ubuntu`) |
| `BBARIT_AGENT_ROOT` | dir Nginx serves at `https://bbarit.com/agent` |

Without them, the Release still publishes to GitHub; only the bbarit.com mirror
is skipped.

## Manual deploy (no CI)

Publish a build straight from your machine:

```sh
export BBARIT_SSH=ubuntu@bbarit.com
export BBARIT_AGENT_ROOT=/var/www/bbarit/agent
FROM_GH=1 ./scripts/deploy-bbarit-com.sh     # pull all platforms from the GH release
# or, ship just your local platform:
./scripts/deploy-bbarit-com.sh
```

## How users get it

```sh
# Fresh install (macOS / Linux)
curl -fsSL https://bbarit.com/agent/install.sh | sh

# Fresh install (Windows, PowerShell)
irm https://bbarit.com/agent/install.ps1 | iex

# Update in place, any time (all platforms)
bbarit-oss --upgrade
```

`bbarit-oss --upgrade` reads `https://bbarit.com/agent/latest.json`, and if a newer
version exists, downloads the matching binary and atomically replaces itself
(on Windows the running exe is moved aside first).

## Nginx (server side)

Serve the agent directory as static files, e.g.:

```nginx
location /agent/ {
    alias /var/www/bbarit/agent/;
    autoindex off;
}
```

`install.sh`, `install.ps1`, `latest.json`, and `dist/<version>/bbarit-<target>` then live under
`https://bbarit.com/agent/`.

## Local build

```sh
cargo build --release        # → target/release/bbarit
cargo test                   # run the suite
cargo fmt --all --check      # CI enforces formatting
```
