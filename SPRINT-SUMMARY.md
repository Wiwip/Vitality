# SPRINT-SUMMARY — sprint/2026-11-07 (cycle 3, base `auto/2026-11-07`)

Stopped because the queue emptied: both scout items landed, none skipped. Cycle 1 did the migration, cycle 2 did the lint pass and the loud errors; cycle 3 closed the two remaining "landed but untested" gaps that cycle 2's own scout-style review had named. Scout this cycle ran with an explicit instruction that an empty result was acceptable, and found two things worth queuing with mutation evidence rather than inventing a third.

| item | commit | verdict | one line |
| --- | --- | --- | --- |
| T-5 pin the guarded `ScenePlugin` install | `b65d457` | OK (after one fix pass) | a host that only called `init_asset::<ScenePatch>()` now provably still gets `ScenePlugin` |
| T-6 make the pre-update grant test pin the whole grant chain | `fcbc69f` | OK | the pre-update grant now fails if `apply_scene` is deleted, and a double machine start is caught |

Gate at the tip: `bash scripts/gate.sh` exits 0, 11 tests (5 lib + 5 ability + 1 systems), confirmed on runs that printed `Compiling vitality v0.2.0`.

## Reviewer notes the owner should read

- **The `ScenePlugin` guard is pinned, but not against the edit most likely to break you.** `tests/test_ability_machine.rs::scene_plugin_is_installed_when_the_host_only_inited_the_asset` catches a return to the old resource predicate and catches deleting the install. It does **not** catch deleting the guard while leaving `app.add_plugins(ScenePlugin)` unconditional — and that is the variant that panics a host which adds `DefaultPlugins` or `ScenePlugin` itself, with `AppError::DuplicatePlugin`. No test in `tests/` builds an app that adds `ScenePlugin` before vitality, so that host is untested; a scratch app doing so is the cheap way to see it.
- **One claim in the item text was wrong and is now wrong nowhere but there.** The scout wrote that `debug_overlay.rs`'s `spawn_scene` queues through `QueuedScenes`. It does not: `commands.spawn_scene(..)` queues a closure calling `entity.apply_scene(scene)` (`bevy_scene-0.20.1/src/spawn.rs:361-373, 488-493`), which needs `AssetServer` + `Assets<ScenePatch>`; only the `queue_*` variants touch `QueuedScenes`, and `grep -rn "QueuedScenes\|WaitingScenes" src/` prints nothing. The guard protects both scene sites by guaranteeing `ScenePlugin::build`'s `init_asset::<ScenePatch>()` runs. The test says that now; `SPRINT-TODO.md`'s T-5 text carries a correction note because it was already landed.
- **The pre-update grant path is now pinned end to end, with one hole left.** `machine_definition_is_installed_before_the_first_update` fails if `src/ability/command.rs:26` is deleted, and fails on a second `StateEntered<AbilityMachine>`. It still passes if something applies a hardcoded one-subtask scene ignoring `ability_def.task_scene`, and nothing observes `StateEntered<TaskMachine>` on that path, so a duplicated subtask start slips through.
- **`test_app` cannot serve a pre-update grant, and that is a fixture constraint worth knowing.** The helper's `Startup` pair consumes `HeldAbility`; a pre-update grant consumes it first, so reusing the helper panics with "the ability is registered once". A passing reuse exists (register through a direct `RegistryMut` system) but re-implements `register_ability` and leaves the helper free to spawn a second `TestActor`. If you want one bootstrap for all five tests, the helper needs a variant that does not install the `Startup` pair.

## Edge cases

- T-5: assert 3 (`Assets<ScenePatch>` still present) cannot fail today, since both `init_asset` and `ScenePlugin::build` insert only if missing; it is kept as a no-clobber check.
- T-5: moving the guard later inside `AttributesPlugin::build` has no observable consequence, since the scene resources are consumed at grant time, not build time.
- T-6: if `Tasks` ever drops out of `Ability`'s `#[require]` list, `sub_tasks`'s `.expect` panics instead of failing an assertion, so the test reports a panic rather than a diff.
- T-6: `Entered` is a per-app counter, so any future second grant added to that test trips the vector for the wrong reason.
- all: still true from cycle 1 — this branch does not build on its own, because the bevy `0.20` bump and `hfsm_bevy = { path = "../hfsm-rs/crates/hfsm_bevy" }` live in your uncommitted `Cargo.toml`, which `land.sh` never stages.

## Try it

1. Break the scene guard and watch the test catch it. In a scratch copy, change `src/lib.rs:103` to `if !app.world().contains_resource::<Assets<ScenePatch>>()`, run `cargo test --test test_ability_machine`, and read `AttributesPlugin must install ScenePlugin even when Assets<ScenePatch> already exists`. Then try the sneakier edit — delete the `if`, keep the `add_plugins` — and notice nothing fails: build an app that adds `ScenePlugin` before `AttributesPlugin` and you get `AppError::DuplicatePlugin` instead.
2. Break the grant chain: delete `commands.entity(trigger.entity).apply_scene(scene);` from `src/ability/command.rs:26` and run the same file. `machine_definition_is_installed_before_the_first_update` now fails with `left: 1 / right: 0`; before this sprint it passed, which was the hole.
3. See the double-start catch: add a second `grant(&mut app);` before the test's single `app.update()` and watch the `Entered` assert fail with two entries.

## Retro

- Costliest: the two items together are 37 added lines and consumed three worker runs, two reviews and five gate cycles. Test items are the worst ratio in this loop so far, because the useful work is the mutation evidence, and the mutation evidence is entirely in the child's mirror — nothing lands except the assertion.
- Blocked: nothing, but the mirror's mtime hazard cost the controller a landing this cycle: a scout left a mutated build cached, `tar` restored clean sources with older mtimes, cargo replayed the mutated binary, and `land.sh todo` failed four tests and committed nothing. Diagnosed with `cmp`, fixed by touching sources; the procedure is now "touch the repo's sources immediately before every `land.sh` call", and `gate.sh` still wants `--touch` — a file this loop will not edit on its own.
- Skipped or retried: none. Two item texts were wrong and were corrected at landing: T-5's `QueuedScenes` mechanism claim, and T-6's assumption that the `StateEntered` observer came free with `test_app` reuse. Cycle 2's proposed process change (run any check phrased as an exact count before the item lands) would not have caught either; these were mechanism claims, not counts. The stronger version is: any item text that asserts *why* code behaves as it does gets one line of source citation (`path:line`), which is exactly what the reviewer used to disprove both.
