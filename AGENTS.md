# AGENTS.md

Vitality: Bevy gameplay-attribute framework (attributes, effects, abilities).

## Build
- `cargo check --offline`
- `cargo test --offline`
- Path deps: `../express-it`, `../hfsm-rs`, declared once in the root `[workspace.dependencies]`. Don't edit them.
- Workspace (dependencies point down only): `vitality_core` (attribute trait, macros, calculator, view traits, roles) <- `vitality_effects` (effect and modifier model + runtime, conditions, effect registry) <- `vitality_abilities` (ability model + runtime, state machines, tasks) <- `vitality_debug` (bevy_ui overlay) <- `vitality` (facade: actors, `Vitality` param, combined registry, plugin, examples, benches, integration tests).
- Each crate defines its own attribute view (`EffectsRef`/`AbilityRef`) that excludes only the components it owns; core code stays generic over `ActorRead`/`ActorWrite`.
- Don't touch `rust-toolchain.toml` or `.cargo/`.

## Comments
- Write like a human dev: short, lowercase-ish, only where the code isn't obvious.
- Explain why, never what.
- One line max. No banners, no doc essays, no restating the signature.
- Skip comments on trivial code.
- Don't add comments to code you didn't change.

## Replies
- Short. Say what changed and anything that needs attention. Nothing else.

## Sprint

A sprint is a branch `sprint/<date>` on which an agent works through the owner's `SPRINT-TODO.md`, one item at a time and test first. It commits each finished item only with `~/.pi/agent/sprint/land.sh`, which reads the block below from the base branch, refuses the protected paths, runs the gate, and commits with the hooks `core.hooksPath` names (none if it is unset). An agent in a sprint never runs `git commit`, `push`, `reset`, `rebase`, `stash` or a branch deletion, never passes `--no-verify`, and never edits a protected path or adds a dependency. The owner reviews the branch before anything is merged.

```sprint
base      = auto/2026-11-09
gate      = scripts/gate.sh
max_items = 4
max_hours = 6
protected = .cargo/**
protected = rust-toolchain.toml
protected = .agent-sandbox
protected = .idea/**
protected = Cargo.toml
protected = crates/*/Cargo.toml
protected = scripts/**
```

`scripts/gate.sh` runs `cargo test --offline --locked --workspace --lib --tests` and then `--doc` per package in the repo. Cargo.toml and
Cargo.lock are read-only in the sandbox, but `--locked` needs no writes, and sccache is shared, so no mirror or
second `CARGO_HOME` is needed. Never write a wrapper script to get around the read-only mounts: if
`--locked` fails, the lockfile is out of date; report it and stop.
