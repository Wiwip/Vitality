#!/usr/bin/env bash
# Gate: test the workspace in place. Cargo.lock is already satisfied, so --locked works on the
# read-only mount; sccache (shared via ~/.cache/sccache) makes cold dependency builds cheap.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --offline --locked --workspace --lib --tests
# cargo test cannot see a lint: an `assert!(n <= 0)` on a usize passes the tests and is a
# deny-by-default clippy error, and the test targets only show up under --all-targets
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
# examples carry required-features = ["examples"], so no test target ever compiles them
cargo check --offline --locked -p vitality --examples --features examples
# per package: --workspace unifies bevy dynamic_linking (D-22) and the doctest binaries cannot load libstd.so
for p in vitality_core vitality_effects vitality_abilities vitality_debug vitality; do
  cargo test --offline --locked -p "$p" --doc
done
