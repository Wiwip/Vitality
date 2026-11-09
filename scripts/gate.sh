#!/usr/bin/env bash
# Gate: test the workspace in place. Cargo.lock is already satisfied, so --locked works on the
# read-only mount; sccache (shared via ~/.cache/sccache) makes cold dependency builds cheap.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
exec cargo test --offline --locked --workspace --lib --tests
