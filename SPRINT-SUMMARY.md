# SPRINT-SUMMARY — sprint/2026-11-07 (base `auto/2026-11-07`)

Stopped because the queue emptied: all four owner items landed, nothing was skipped, nothing was parked mid-flight.

| item | commit | verdict | one line |
| --- | --- | --- | --- |
| G-1 make the gate green again on bevy 0.20 and the current hfsm API | `992831b` | OK after a BLOCK | 23 compile errors on the bumped manifest → 0, whole library migrated |
| C-1 make examples and benches build on bevy 0.20 | `5be0992` | OK | one break, in `examples/abilities.rs`; the other three targets already compiled |
| C-2 install each machine with `add_state_machine` | `e8d98d7` | OK | fixes a real pre-update grant bug, not just tidiness |
| C-3 pin the ability and task machines with headless tests | `281750f` | OK | `tests/test_ability_machine.rs`, 3 tests, 5 negative controls all reproduced |

Gate at the tip: `bash scripts/gate.sh` exits 0 — 5 lib + 3 ability/task + 1 integration test, verified from a `cargo clean -p vitality` rebuild.

## Reviewer notes the owner should read

- **Four test setups were changed** (three in `src/`, one in `tests/test_systems.rs`), each one line: `app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1))`. Why: on bevy 0.20 a headless `app.update()` lasts ~40us against `Time<Fixed>`'s 15 625us timestep, so `FixedUpdate` — which hosts vitality's whole attribute pipeline — ran zero times and four tests failed on real clock time. Both reviewers deleted the lines to confirm they are load-bearing, and no assertion was changed, skipped or weakened. Real apps run at frame length and are unaffected; **headless or test apps are not** — a user running a headless server sees attributes silently stop updating.
- **`AttributesPlugin` now installs `bevy::scene::ScenePlugin`** when the app has none (`src/lib.rs`, guarded by `is_plugin_added`). Without it, granting an ability panics `Requested resource Assets<ScenePatch> does not exist`, because bevy 0.20 spawns through `ScenePatch` assets. The first version of this line was an unconditional add and the reviewer BLOCKed it: it panicked every `DefaultPlugins` app, both examples included.
- **`dispatch_event` failures are now logged and ignored; `is_in_state` failures read as `false`** (9 call sites). That is the public-API consequence of hfsm returning `Result` now. One known quiet spot: hfsm's own 0.19 `is_in_state` logged before returning `false`, so a `Tasks` entry missing `MachineInstance<TaskMachine>` now pins `Complete::All` to false silently.
- **`examples/abilities.rs` changed shape for the same codegen**: `bsn!` re-reads a bare `fn(args)` line as a *template* call in 0.20, so scene calls are now `@task::<DebugLongTask>(NoData)` with `--` separators. The reviewer checked the macro sources: `@fn(args)` emits the same `SceneScope(..)` 0.19's bare form emitted, token for token; tree shape, names, arguments and relationship order unchanged.
- **Two `pre-existing bug` findings came out of the test work** (see Edge cases 1 and 2), both from a reviewer reproducing them with mirror probes rather than from a failing test.

## Edge cases

- C-2: a host that adds `AttributesPlugin` *before* `DefaultPlugins`/`ScenePlugin` takes vitality's scene install and then panics on its own later `add_plugins(ScenePlugin)` with "plugin was already added in application". Both examples add `DefaultPlugins` first, so nothing in the repo hits it.
- C-2: a host that wires `resolve_scene_patches`/`spawn_queued` by hand without adding `ScenePlugin` now gets the plugin on top and runs the scene-spawn systems twice.
- C-2: adding `AttributesPlugin` and `AbilityPlugin` in one app panics at add time — every vitality plugin is unique (`bevy_app-0.20.1/src/plugin.rs:89-91`) and `AttributesPlugin::build` already adds the other five. Pre-existing (same default in 0.19), and no example or test does it.
- C-3: nested `Tasks` never activate downward. `ActiveState::on_enter` dispatches `TaskEvent::Activate` only to direct `tasks` (`src/ability/ability_state.rs:216-224`) and the only other downward dispatch is `CompleteState::on_enter` (`src/ability/task_states.rs:127-137`), i.e. after the parent already completed — so a compound task gated on `Complete::All` leaves its children `Pending` forever. Reviewer probe on `Tasks [ LongTask -- LongTask { leaf: InstantTask } ]`: `pending_leaves=1/1 ability_active=true`. `examples/abilities.rs:66-79` builds exactly that shape, so this is intended usage that is broken. Pre-existing, untouched by this sprint.
- C-3: `Vitality::get_global_actor()` (`src/context.rs:196`) does `.single().unwrap()` on `GlobalActor`, which `GlobalEffectPlugin` only spawns in its own `PreStartup`, so spawning a real actor before the first `app.update()` panics. The new tests need a `TestActor` marker for the same reason `spawn_global_actor` gives the global actor entity a real `Actor`: any `Single<Entity, With<Actor>>` then finds two entities.
- C-3: `two_completion_notifications_enter_recovery_once` counts `StateEntered<AbilityMachine>` and guards on two `EndAbility` deliveries, but a future change that drops the second notification instead of ending twice would pass it silently.
- C-1: `benches/bench_systems.rs` compiles, but its bench bodies are commented out (lines 16-32, 37-58, 62-76), so `cargo bench` measures nothing and carries 15 dead-code warnings. Pre-owner code.
- all: this branch does not build on its own. The bevy `0.20` bump and the `hfsm_bevy = { path = "../hfsm-rs/crates/hfsm_bevy" }` line live in the owner's uncommitted, read-only `Cargo.toml`, which `land.sh` never stages (it is in `.git/sprint-foreign`). `HEAD` of `main` is already un-buildable for the same class of reason (`path = "../hfsm_bevy"` no longer exists), so the branch only builds once that file is committed alongside it.

## Try it

1. See the migration's visible result — the ability example, which is the only thing in the repo that drives a full ability through the UI: `cargo run --example abilities` (needs a display). It should spawn the actor, grant and run abilities, and show the debug overlay; a panic naming `Assets<ScenePatch>` or `plugin was already added` would mean the scene install or the plugin graph regressed.
2. See the tests that did not exist an hour ago, and that they are not vacuous: `cargo test --test test_ability_machine -- --nocapture`, then in a scratch copy make `RecoveryState::on_enter` arm no timer and watch all three fail.
3. See the bug C-2 fixed: in a scratch test, add `MinimalPlugins + AssetPlugin + AttributesPlugin`, create an `Ability` bundle (it carries `#[require(…, MachineInstance<AbilityMachine>)]`) **before** the first `app.update()`, and grant it. On this branch that works; on `HEAD~2`-style installs it died with `MachineQuery<AbilityMachine>::view failed validation: Resource does not exist`.

## Retro

- Most expensive by far: getting one trustworthy build loop. `Cargo.toml`, `Cargo.lock` and `~/.cargo/registry` are read-only and there is no network, so cargo cannot run in the repo at all; the mirror gate costs a one-time cold dependency build and every child rediscovered it. Second: G-1's three fix passes (gate failure, then a reviewer BLOCK on the scene-plugin line).
- Blocked: nothing stopped. Recurring hazard, hit three times by three different children: `scripts/gate.sh` mirrors with `tar`, which restores old mtimes, so cargo can reuse a stale artifact and report a phantom failure or replay stale warnings — every "green" in this sprint's log is from a run that printed `Compiling vitality`.
- Skipped or retried: none. One item was BLOCKed and then cleared inside the same item.
- Proposed process change (proposal only, not applied): have `scripts/gate.sh` extract with `--touch` (or `touch` the mirrored sources) so a gate run can never reuse a stale artifact. It would also let go of the controller procedure this sprint had to adopt — touching every source before each landing gate run.
