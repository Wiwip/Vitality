# SPRINT-TODO

The owner asked for the council migration plan to be executed: vitality onto bevy 0.20.1 and the
current `hfsm-rs` (`hfsm_core` + `hfsm_bevy`). `Cargo.toml` already carries the version bump (it is
the owner's uncommitted edit, read-only in this sandbox and listed in `.git/sprint-foreign`), so
nothing here changes a dependency. The source has never been compiled against either target.

Evidence for every line below was read from `../hfsm-rs` and from the pinned `bevy-0.20.1` /
`bevy_ecs-0.20.1` / `bevy_ecs_macros-0.20.1` crate sources in `~/.cargo/registry`.

## G-1: make the gate green again on bevy 0.20 and the current hfsm API
- change: the gate is red on the base — `scripts/gate.sh` cannot even resolve (`Cargo.lock` pins 15 crates the read-only cache lacks, e.g. `async-executor 1.14.1`, `cc 1.11.1`, `uuid 1.29.1`, `winnow 1.0.5`), which `scripts/gate.sh` already works around by resolving offline in a mirror; what is left is the source migration. Fix, in this order:
  1. `On<Add, T>` / `On<Insert, T>` → `On<Add<T>>` / `On<Insert<T>>` — `Add<B: Bundle>` is bundle-generic in bevy 0.20 (`bevy_ecs-0.20.1/src/lifecycle.rs:352`; the whole of hfsm-rs's own 0.19→0.20 port, commit `f589b06`, is this one change). Sites: `src/attributes.rs:400`, `src/effect/global_effect.rs:38`, `src/ability/tasks.rs:98`, `src/ability/command.rs:7`.
  2. `VariantDefaults` is gone in 0.20 (absent from `bevy_ecs` and `bevy_ecs_macros` 0.20.1); `FromTemplate` now handles enums and takes the `#[default]` variant (`bevy_ecs_macros-0.20.1/src/template.rs:125-150`). `src/graph.rs:2,18` — drop the derive and the import; `NodeType` already has `Default` + `#[default] Actor`.
  3. `StateTimer::set_timer` takes `std::time::Duration` first (`hfsm_bevy/src/lib.rs:1232-1243`); `AbilityDef.cooldown` is `StoredExpr<f32,…>` (`src/assets.rs:54`), so `src/ability/ability_state.rs:254` converts seconds with `Duration::from_secs_f32`.
  4. `MachineEvent<T>` has a private `machine: PhantomData<fn() -> T>` field (`hfsm_bevy/src/lib.rs:1042-1050`), so every struct literal is a hard error — use `MachineEvent::new(entity, event)` at all 6 sites: `src/ability/tasks.rs:150,472,482,492`, `src/ability/task_states.rs:134`, `src/context.rs:118`.
  5. `MachineQuery::start`/`dispatch_event`/`active_states`/`is_in_state` now return `Result<_, BevyError>` (`hfsm_bevy/src/lib.rs:375,473,568,580`). Sites: `is_in_state` at `src/ability/system_param.rs:82`, `src/ability/ability_state.rs:264`, `src/ability/tasks.rs:238,241`; `dispatch_event` at `src/ability/system_param.rs:147,164`, `src/ability/ability_state.rs:223,268,277`. Per D-3: an `is_in_state` `Err` means "this entity holds no machine instance", so read sites keep the old tolerance (`unwrap_or(false)`); a `dispatch_event` `Err` is logged with the entity and machine type and then ignored — no `unwrap()`, no silent `let _ =`.
  6. The private-field break that forces this item to be one commit: `src/ability/tasks.rs:233` reads `ability_machines.view.task_machines`, and `MachineQuery`'s fields are private now (`hfsm_bevy/src/lib.rs:263-290`, owner decision C-39). In this system `MachineQuery<AbilityMachine>` has exactly one consumer — the trailing `dispatch_event` at `tasks.rs:252` — so per D-6 make `MachineQuery<TaskMachine>` the only machine parameter, add a `mut commands: Commands`, and end the ability with `commands.trigger(MachineEvent::<AbilityMachine>::new(trigger.entity, AbilityEvent::EndAbility))`. `trigger.propagate(result)` and the `Complete::All`/`Any` judgement stay as they are. Do not add a second `MachineQuery<TaskMachine>` beside `MachineQuery<AbilityMachine>` anywhere: `AbilitySystemParam` already nests one (`src/ability/ability_state.rs:70`) and both would borrow `Write<MachineInstance<TaskMachine>>`.
  7. Whatever else the mirror reports in `src/` until `cargo test --offline --lib --tests` exits 0 — expected: `src/inspector/debug_overlay.rs:34` passes a `&str` where `FontSource::Handle` now carries `Handle<Font>` (`bevy_text-0.20.1/src/text.rs:283-296`).
  Leave `src/systems.rs` and `Cargo.toml` alone (`.git/sprint-foreign`): its `On<MarkNodeDirty<T>>` / `On<UpdateAttributeSignal<T>>` triggers are user-event forms and do not change.
- test: none (the gate is the check). Check: `scripts/gate.sh` exits 0; before, it dies on `error: failed to download async-executor v1.14.1` / `failed to unpack ar_archive_writer` in the repo and on the API errors above in the mirror.
- origin: owner
- status: done

## C-1: make examples and benches build on bevy 0.20
- change: G-1 corrected this item's premise — `--lib --tests` does build and run `tests/test_systems.rs`, which is why G-1 owns its one clock line — so what is left un-migrated is `examples/simple.rs`, `examples/abilities.rs`, `examples/plan.rs` and `benches/bench_systems.rs`. Fix what bevy 0.20 and the hfsm change break there (known: `examples/simple.rs:177,200` and `examples/abilities.rs:52` use `On<ExecuteAbility>` — user events, unchanged — so expect the breaks to be UI/template and `Abilities`-API shaped; `benches/bench_systems.rs` has `harness = false`, so it never enters the gate). Compile-only is the bar: examples need a display (`bevy_winit` panics with no `DISPLAY`), so nothing here has to run.
- after: G-1
- test: none (the gate runs lib + `tests/` only). Check: sync the repo into `/tmp/vitality-build` and run `cargo check --offline --all-targets` there — exits 0, no errors; `examples/` and `benches/` compile.
- origin: owner
- status: done

## C-2: install each machine with `add_state_machine`
- change: also take the reviewer's better scene-plugin predicate from G-1: `src/lib.rs` guards `ScenePlugin` with `!app.world().contains_resource::<Assets<ScenePatch>>()`, which mis-reads a host that called `init_asset::<ScenePatch>()` itself (it then skips the install and `queue_apply_scene` panics on `QueuedScenes`, `bevy_scene-0.20.1/src/spawn.rs:500`); `!app.is_plugin_added::<ScenePlugin>()` is precise. Then: `src/ability/mod.rs:36-37` adds `StateMachinePlugin::<AbilityMachine>` and `StateMachinePlugin::<TaskMachine>`, while the definitions are inserted separately from `setup_ability_machine_definition` (`src/ability/ability_state.rs:107`) and `setup_task_machine_definition` (`src/ability/task_states.rs:91`). `App::add_state_machine::<T>(definition)` inserts the `MachineDefinition<T>` resource and the plugin together (`hfsm_bevy/src/lib.rs:178-201`), so the definition can never be missing when the plugin's `on_insert` observer fires for a `#[require(MachineInstance<T>)]` bundle. Move both installs to `add_state_machine` in `AbilityPlugin::build` and delete the two resource-inserting setup functions and wherever they are scheduled. Bounds hold: `T::Event: Clone + Debug` is already derived by `AbilityEvent` and `TaskEvent`.
- after: G-1
- test: none (a pure install refactor with no new behaviour). Check: no `add_plugins(StateMachinePlugin` and no `insert_resource(build_machine` remain in `src/`; the gate stays green; `examples/abilities.rs` still compiles.
- origin: owner
- status: done

## C-3: pin the ability and task machines with headless tests
- change: nothing in the repo drives an ability or task machine — `tests/test_systems.rs` covers attributes/effects only — so G-1's behavioural change at `src/ability/tasks.rs` and the `Duration` cooldown have no regression net. Add `tests/test_ability_machine.rs` on `MinimalPlugins` + `Time` + `StateMachinePlugin`/`add_state_machine` (no `bevy_ui`, no asset server, so it runs headless): drive one ability `Ready → Active → Recovery → Ready` through `MachineEvent<AbilityMachine>`, assert `StateTimer::is_armed(AbilityState::Recovery)` after `EndAbility` and that `Recovered` follows when the clock passes the cooldown, and assert that two `NotifyTaskCompletion` notifications in one update produce exactly one `StateEntered<AbilityMachine>` for `Recovery` (the case G-1's deferred `EndAbility` trigger decides).
- after: G-1
- test: `cd /tmp/vitality-build && cargo test --offline --test test_ability_machine` — each assertion above fails when the route or the cooldown it pins regresses; today the file does not exist, so there is no coverage at all.
- origin: owner
- status: done
