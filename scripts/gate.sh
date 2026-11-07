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

# member manifests change as crates move, so the lockfile check hashes all of them
manifest_sum() { (cd "$1" && find . -name Cargo.toml -not -path './target/*' | sort | xargs cat | sha256sum); }

mkdir -p "$mirror"
# The path deps live outside the repo, so the mirror needs the same siblings next to it.
parent=$(dirname "$mirror")
# Link the sibling checkout each path dep starts in, so ../<name>/... resolves the same way here.
while read -r dep; do
  case $dep in
    ../*) name=$(printf '%s\n' "$dep" | cut -d/ -f2) ;;
    *) continue ;;
  esac
  link=$parent/$name
  { [ -e "$link" ] || [ -L "$link" ]; } || ln -s "$src/../$name" "$link"
done < <(find "$src" -name Cargo.toml -not -path '*/target/*' -exec sed -n 's/.*path[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' {} + | sort -u)

# Copy everything but build output and the lockfile: the mirror resolves its own offline graph.
tar -C "$src" \
  --exclude=./target \
  --exclude=./.git \
  --exclude=./.auto \
  --exclude=./.env \
  --exclude=./Cargo.lock \
  -cf - . | tar -C "$mirror" -xf -

cd "$mirror"
export CARGO_HOME=$home

if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile --offline
elif [ "$(manifest_sum "$src")" != "$(cat "$mirror/.gate-manifest" 2>/dev/null)" ]; then
  cargo update --offline
  manifest_sum "$src" > "$mirror/.gate-manifest"
fi

exec cargo test --offline --workspace --lib --tests
