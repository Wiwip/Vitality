#!/usr/bin/env bash
# Gate: build and test vitality in a writable mirror of the repo.
#
# This sandbox mounts Cargo.toml, Cargo.lock and ~/.cargo/registry read-only and has no
# network, so cargo cannot resolve or build inside the repo at all: it has to rewrite the
# lockfile and unpack crates. The mirror is a copy under /tmp that owns its own lockfile
# and target/, built against the same pinned sources. The repo stays the only place code
# is edited.

set -euo pipefail

src=$(git rev-parse --show-toplevel)
mirror=${VITALITY_MIRROR:-/tmp/vitality-build}
home=${VITALITY_CARGO_HOME:-/tmp/ch}

# cargo needs a writable CARGO_HOME; the crate cache and index it reads stay the read-only ones.
mkdir -p "$home/registry"
for dir in cache index; do
  [ -e "$home/registry/$dir" ] || ln -s "$HOME/.cargo/registry/$dir" "$home/registry/$dir"
done

mkdir -p "$mirror"
# The path deps live outside the repo, so the mirror needs the same siblings next to it.
parent=$(dirname "$mirror")
while read -r dep; do
  target=$src/${dep#./}
  [ -d "$target" ] || continue
  link=$parent/$(basename "$dep")
  { [ -e "$link" ] || [ -L "$link" ]; } || ln -s "$target" "$link"
done < <(sed -n 's/.*path[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$src/Cargo.toml")

# Copy everything but build output and the lockfile: the mirror resolves its own offline graph.
tar -C "$src" \
  --exclude=./target \
  --exclude=./.git \
  --exclude=./.auto \
  --exclude=./Cargo.lock \
  -cf - . | tar -C "$mirror" -xf -

cd "$mirror"
export CARGO_HOME=$home

if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile --offline
elif ! cmp -s "$src/Cargo.toml" "$mirror/.gate-manifest"; then
  cargo update --offline
  cp "$src/Cargo.toml" "$mirror/.gate-manifest"
fi

exec cargo test --offline --lib --tests
