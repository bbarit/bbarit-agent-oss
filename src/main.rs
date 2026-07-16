//! Thin binary for the bbarit-oss coding agent: all logic lives in the
//! library (`src/lib.rs`); this just forwards to `run()`.
fn main() -> anyhow::Result<()> {
    bbarit_oss::run()
}
