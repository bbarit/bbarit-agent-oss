# Contributing to bbarit-agent

Thanks for your interest — contributions are very welcome, from typo fixes to new
providers and tools.

## Getting started

```sh
git clone https://github.com/bbarit/bbarit-agent-oss
cd bbarit-agent
cargo build
cargo test
./target/debug/bbarit --help
```

You need a stable [Rust toolchain](https://rustup.rs). No other services are
required to build or run.

## Before you open a pull request

Please make sure the same checks CI runs pass locally:

```sh
cargo fmt --all        # must be clean (CI runs `cargo fmt --check`)
cargo build
cargo test
cargo clippy           # advisory — please don't add new warnings
```

- Keep changes focused; one topic per PR.
- Add or update tests for behavior changes.
- Match the surrounding code style — prefer explicit over clever.
- Comments explain *why*, not *what*; write them in English.
- Update `README.md` / `CLI.md` when you change user-facing behavior.

## Good first contributions

- New LLM providers or models in `src/providers/`.
- New tools in `src/tools.rs`.
- Additional personas under `personas/`.
- Bug fixes with a regression test.
- Documentation improvements.

## Reporting bugs & requesting features

Open an issue using the templates. For bugs, include your OS, `bbarit-oss --version`,
the command you ran, and what happened vs. what you expected.

## Provenance & licensing

bbarit-agent is MIT-licensed and based on [Pi](https://github.com/earendil-works/pi)
(MIT); see [PROVENANCE.md](./PROVENANCE.md). By contributing, you agree that your
contributions are licensed under the MIT license. Do not paste code from
non-MIT-compatible sources.

## Code of conduct

This project follows the [Contributor Covenant](./CODE_OF_CONDUCT.md). Be kind.
