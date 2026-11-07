# SPRINT-SUMMARY — sprint/2026-11-07 (cycle 2, base `auto/2026-11-07`)

Stopped because the queue emptied: the four scout items landed, none skipped, none parked mid-flight. This sprint is the tail of the owner's plan — stage 7's lint pass, plus the two gaps cycle 1's own verdict recorded.

| item | commit | verdict | one line |
| --- | --- | --- | --- |
| T-1 make `cargo fmt --check` exit 0 | `e30d515` | OK (verdict line missing from the review, see Retro) | one import in `src/context.rs`; the gate had been red since before the migration |
| T-2 clear the two lints the migration left | `bc0911c` | OK | 4 redundant `Send` bounds gone, last deprecated BSN comma gone, library now builds warning-free |
| T-3 pin the `add_state_machine` install with a pre-update grant test | `f98dc72` | OK | closes the one cycle-1 fix that had no committed test |
| T-4 log the `is_in_state` error the completion judgement swallows | `0d4667b` | OK | the silent `Complete::All` pin now names the entity, tolerance unchanged |

Gate at the tip: `bash scripts/gate.sh` exits 0, 10 tests (5 lib + 4 ability/task + 1 systems), confirmed on a run that printed `Compiling vitality v0.2.0`.

## Reviewer notes the owner should read

- **`is_in_state` failures are no longer silent, but they are best-effort.** Four hfsm reads went from `unwrap_or(false)` to `inspect_err(…).unwrap_or_default()`; the reviewer proved with live tracing probes that each line fires, and proved on both trees that the tolerated `false` is unchanged. Two measured limits: `all()`/`any()` short-circuit, so a broken subtask visited after a healthy one is never read and never logged, and `warn_once!` is once per process per call site, so only the first lost `MachineInstance<AbilityMachine>` is ever named.
- **Two public builder signatures narrowed**: `AbilityBuilder::add_execution`/`add_trigger` and `EffectBuilder::add_effect_trigger`/`add_actor_trigger` dropped a redundant `+ Send`. The reviewer checked the accepted set rather than just compilation: every `T: IntoObserverSystem` is already `Send` via the trait supertrait, so the set is equal, not larger — no caller can newly fail and none newly passes. Worth a line in a changelog.
- **`tests/test_ability_machine.rs` now carries a stand-in `GlobalActor`** it despawns before the update. Reason, reproduced: `observe_spawned_actor` (`src/effect/global_effect.rs:38`) fires on `Add<Actor>` and unwraps `Vitality::get_global_actor()` (`src/context.rs:196`), so spawning an actor in an app that has not run an update yet panics. The reviewer confirmed the despawn is load-bearing (keeping it gives `MultipleEntities`) and that moving it after the grant still passes, so the pinned path is the real one. This is the third time that `unwrap()` surfaced in this run; it is an owner decision, not sprint work.
- **`cargo fmt --all` is still not safe to run blindly** here: it would rewrite the owner's `src/systems.rs` if that copy ever drifts, so both lint items used `rustfmt --edition 2024 <file>`. Your `src/systems.rs` is rustfmt-clean today, so it would not move now.

## Edge cases

- T-2: reaching a clippy grep of 0 for `implied_bounds_in_impls` needs `Copy` removed at `src/ability/builder.rs:48` and `src/actors.rs:110` (both pre-migration, one outside that item's files). Any future stage-7 grep asserting 0 goes red until then.
- T-2: the library warning count moved 83 → 77 on a 5-line diff, because clippy's `let_unit_value` warning on the BSN macro's generated `let _ = warning;` disappeared with the deprecation marker block. Anything baselining 83 will see a 6-warning drop.
- T-2: `Children [ Text(..) TextFont{..} DebugOverlayMarker ]` now has no separator at all, so appending `Node {..}` there lands it on the same child silently — BSN splits entities only on `--`.
- T-3: the new test asserts only `Ready` after a pre-update grant, so a regression that drops the `Tasks` scene application on that path (children never spawned) still passes; it also registers no `StateEntered` observer, so a double machine start would pass.
- T-3: the stand-in `GlobalActor` carries no `Actor`, unlike the real one, so a future `GlobalEffects` entry that reads the global actor's attributes would run against a component-less entity in that test where a real app would not.
- T-4: `is_ability_ready` has no caller anywhere in the repo, so its `warn_once!` volume is a host-side property that cannot be measured here; and a host that triggers `NotifyTaskCompletion` itself every frame would print one line per broken subtask per frame.
- all: still true from cycle 1 — this branch does not build on its own, because the bevy `0.20` bump and the `hfsm_bevy = { path = "../hfsm-rs/crates/hfsm_bevy" }` line live in the owner's uncommitted `Cargo.toml`, which `land.sh` never stages.

## Try it

1. Watch the thing T-4 fixed actually speak. In a scratch test, grant an ability with two subtasks, remove `MachineInstance<TaskMachine>` from one, complete the other, and run the update with `MinimalPlugins` + `bevy_log::LogPlugin` — you now get `[<entity>] TaskMachine is_in_state failed: is_in_state: no machine instance for entity …`; before this sprint the ability just never ended, in silence.
2. See the library build clean: `cargo check --lib` prints no warning line at all, and `cargo fmt --all -- --check` exits 0 (it exited 1 with two hunks on every commit before this sprint, including `ecaa458`).
3. See the install pin: `cargo test --test test_ability_machine`, then in a scratch copy restore `add_plugins(StateMachinePlugin::<AbilityMachine>…)` plus a `PreStartup` definition insert and watch `machine_definition_is_installed_before_the_first_update` fail with "the AbilityMachine definition must come from the plugin, not from a system".

## Retro

- Costliest: verification cost per item is roughly constant while the items got smaller. T-1 is a one-import formatting change and still cost a worker run, a review and a gate cycle; the gate itself is ~1 minute warm, but each child re-establishes the mirror and the mtime discipline before it can trust a green.
- Blocked: nothing. Recurring hazard unchanged from cycle 1: `scripts/gate.sh` mirrors with `tar`, which restores mtimes, so a run that does not print `Compiling vitality` can be a replay. Three of four sprint-2 children hit it; the controller now touches every source before a landing gate run.
- Skipped or retried: none. Two item texts were wrong and were corrected at landing rather than re-scoped: T-2's `grep … == 0` (unreachable inside its own scope) and T-3's "no `GlobalActor` is needed" (false, found by running the code).
- Proposed process change (proposal only, never applied): have the item template require that any check phrased as an exact count be run once by the author before the item lands. Both wrong checks this cycle were count assertions written from reading, not from running.
