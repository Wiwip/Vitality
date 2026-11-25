# vitality

Gameplay attributes, effects and abilities for Bevy: one attribute registry, an effect runtime that writes modifiers into it, and an ability state machine on top.

| Crate | Role |
|---|---|
| `vitality_core` | `Attribute` trait and macros (`attribute!`, `tag!`), calculator, attribute registry and resolver tables, view traits, counted tags |
| `vitality_effects` | effects, modifiers, conditions, the attribute resolver, effect registry |
| `vitality_abilities` | abilities, tasks, state machines, ability registry |
| `vitality_debug` | `bevy_ui` inspector overlay (optional) |
| `vitality` | facade: `VitalityPlugin`, actors, `Vitality` system param, examples |

Dependencies point one way: core <- effects <- abilities <- debug <- facade.

```toml
vitality = { path = "crates/vitality", default-features = false } # drops the bevy_ui overlay
```

Tested feature matrix: `--features dyn`, `--features multi_threaded`, `--features dyn,multi_threaded` and `--no-default-features` all pass `cargo test --offline --locked --workspace --lib --tests` on their own (`-p vitality`) and as `--workspace`; `--doc` is green in every combination but `dyn`, whose `dynamic_linking` doctest binaries cannot load `libstd.so`.

## Getting started

```rust,ignore
use bevy::prelude::*;
use vitality::prelude::*;
use vitality::actors::ActorBuilder;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::{attribute, tag};

attribute!(Health, f32);
attribute!(MaxHealth, f32);
attribute!(Power, f32);
tag!(Burning);

#[derive(Component, Copy, Clone)]
struct Hero;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            VitalityPlugin::new()
                .attribute::<Health>()
                .attribute::<MaxHealth>()
                .attribute::<Power>(),
        )
        .add_systems(Startup, spawn_hero)
        // a tick later, though an apply in the spawning system lands too
        .add_systems(Update, apply_burn.run_if(run_once))
        .run();
}

fn spawn_hero(mut vitality: Vitality) {
    vitality
        .add_spawn_actor(
            ActorBuilder::new()
                .name("Hero")
                .insert(Hero)
                .with::<Health>(100.0)
                .with::<MaxHealth>(100.0)
                .with::<Power>(10.0)
                .clamp::<Health>(0.0, MaxHealth::src())
                .build(),
        );
}

fn apply_burn(mut vitality: Vitality, hero: Single<Entity, With<Hero>>) {
    // a 5 s burn that scales with the caster's Power and tags the target while it lasts
    let burn = vitality.add_effect(
        EffectBuilder::every_second_for_duration(1.0, 5.0)
            .with_tag::<Burning>()
            .modify::<Health>(Power::src(), ModOp::Sub, EffectSubject::Target)
            .build(),
    );
    vitality.apply_effect_to_self(*hero, &burn);
}
```

Attributes are components: `Query<&Health>`, `Changed<Health>`, `On<CurrentValueChanged<Health>>`. `Effects` applies, finds and removes effects by entity, definition, tag or source and edits stacks; `Abilities` grants, activates, cancels and revokes.

`attribute_set! { pub struct Vitals { Hp: u32, Mp: u32 } }` declares the members and the `Vitals` bundle carrying them: `VitalityPlugin::new().set::<Vitals>()` (or `init_attribute_set::<Vitals>(app)`) registers them all, and `ActorBuilder::insert(Vitals::new(10, 4))` or `Vitals { Hp: Hp::new(10), ..default() }` puts them on an entity.

Events: `EffectApplied`, `EffectRemoved` (reason), `EffectStacksChanged`, `AbilityActivated`, `AbilityActivationFailed` (`ActivationError`), `AbilityEnded`. `EffectBlocked` means a tag rule. A failed `attach_if` on a persistent apply, or `active_while` on an instant one, is a refusal: no spawn, no event. A persistent `active_while` that misses gates instead: the effect lands applied and stays inactive until it holds (`active_while` in `crates/vitality_effects/src/builder.rs`).

`EffectsSet` runs on `FixedUpdate`: headless apps need a time-advancing `TimeUpdateStrategy`. `handles_wait_task_timers` runs there too, on the fixed clock, so a wait and a cooldown of the same length end on the same tick; the cooldown reads through `cooldown_remaining`.

## Def files

`effect_def_from_file` and `ability_def_from_file` read a def off disk; `effect_def_from_str` and `ability_def_from_str` the same text from a bundle. Every line is `key = value`, `#` comments to end of line, a key may repeat where the def holds a list.

```text
name = Regeneration
policy = every 1.0 for 12.0
modifier = Health add 2 target
```

Effect keys: `name`, `policy`, `tag`, `requires`, `blocked_by`, `cleanses`, `modifier`. Ability keys: `name`, `cooldown`, `tag`, `activation_owned`, `activation_requires`, `activation_blocked_by`, `blocks`, `cancels`.

`policy` is `instant`, `permanent`, `for <seconds>`, `every <seconds>`, `every <seconds> for <seconds>`; absent means permanent. `modifier` is `<attribute> <op> <value> <who>` — op is `add`, `sub`, `mul`, `increase` or `set`, who is `target`, `source` or `effect`.

`effect_def_from_ron` and `ability_def_from_ron` take the same def as RON, and the `_ron_file` wrappers read those off disk: `EffectFile(name: "Regeneration", policy: Every(interval: 1.0, duration: 12.0), modifiers: [Modifier(attribute: "Health", op: Add, value: 2.0, subject: Target)])`. Same fields, same refusals; a failure names the field (`modifiers[0]`, `policy.duration`) where a line file names its line.

A file only names what `DefNames` holds: `init_attribute::<T>()` for an attribute, `DefNames::tag::<T>()` for a tag, both as the short type name. Anything else fails its line (``line 2: no tag `Frost` ``), pinned with the loads in `crates/vitality/tests/test_def_file_load.rs`.

Costs, expressions, clamps, task scenes and observers stay Rust.

## Your own number type

An attribute folds in its own property type: `(base + Σadd − Σsub) × (1 + Σincrease) × Πmul`, one operation at a time, a stacked `Mul` as `powi(stacks)`. Any primitive number works as is.

Implement `AttributeValue` (`zero`, `one`, `add`, `sub`, `mul`, `powi`, `from_f64`, `to_f64`), derive `Reflect`, call `property_number!(MyType)` for expression literals; `attribute!(Gold, MyType)` and `EffectBuilder::modify::<Gold>(..)` take it, clamps included.

Expression arithmetic (`A::src() + B::src()`) needs `num_traits::Num`; def files and the debug overlay read through `to_f64`, so lossy past it. Worked example: `crates/vitality/tests/test_property_types.rs`.

## Wiring your own plugin

What each break looks like in an app (contract: the `vitality` crate-level doc):

- Register attributes with `vitality::init_attribute::<T>()` in `Plugin::build`: `App::update()` never calls `finish`, so registering there fires no value-changed events — a permanent effect applies no modifiers, an instant one warns and drops its write.
- `AttributesPlugin` owns the registry: it precedes anything calling `init_attribute`, which panics at build.
- The 128-attribute cap counts the app, not each crate, and `Stacks` takes a slot: 127 game attributes, the 128th refused by name.
- `AttrId` is a registration index and moves with plugin order: persist the name, never the id. A duplicate short name panics at build.
- Add `EffectsPlugin`: its `build` installs the apply-path nodes (`resolve_system`, `apply_all_modifier_events`, `scan_base_writes`, `apply_all_periodic_effects`) and `init_effect_attribute` installs none — skip it and nothing listens for the apply event. Adding it after build changes nothing.

## Performance

From the `scenario` group of `crates/vitality/benches/bench_systems.rs` at `9a20c87`, 2026-11-22, on this box (8-CPU quota, 1-min loadavg 1.3-2.6 across the runs, no cgroup throttling). Shape: 10 000 actors, 20 attributes and 3 effects each; each figure is the median of three runs of the filtered group, one build single-threaded and one with `--features multi_threaded`.

- `steady`, 10 % of the actors' attributes moved: **807 µs** single-threaded, 13 % spread — **inside the 2 ms ceiling**. Multi-threaded it is 3.382 ms. Ids `scenario/steady/actors=10000/a=20/e=3/{st,mt}-static`.
- `storm`, every attribute of every actor: **7.06 ms** single-threaded, 18 % spread, and **10.3 ms** multi-threaded — over the 8 ms ceiling, and directional, since the three runs read 7.24 / 10.3 / 11.59 ms. Ids `scenario/storm/actors=10000/a=20/e=3/{st,mt}-static`.
- `storm_observed`, `storm` with one attribute watched: **7.58 ms** single-threaded, against 10.66 ms before the resolver recorded changes per attribute instead of once per app. One criterion run each side (`[7.5496 7.5840 7.6280]` and `[10.522 10.664 10.867]` ms), so the ratio is directional. Id `scenario/storm_observed/actors=10000/a=20/e=3/st-static`.
- `idle`, nothing moved at all: **8.68 µs** single-threaded, 15 % spread, and **282 µs** multi-threaded. Ids `scenario/idle/actors=10000/a=20/e=3/{st,mt}-static`.
- Multi-threading costs ticks rather than saving them on all three shapes: 32x on `idle`, 4.2x on `steady`, 1.5x on `storm` (directional). The cost is bevy handing its schedule sets to the 8-thread pool, not a vitality walk — at `idle` nothing fans out, and pinned to one core the threaded tick reads 38.1 µs against 8.3 µs single-threaded on that same core (one run each, `taskset -c 0`). Ids `scenario/{idle,steady,storm}/actors=10000/a=20/e=3/{st,mt}-static`.
- An app can keep that cost for the schedules that earn it: `app.edit_schedule(Update, |schedule| schedule.set_executor(SingleThreadedExecutor::new()))`, the same call on `PreUpdate` and `FixedUpdate` (`bevy::ecs::schedule::SingleThreadedExecutor`; bevy 0.20 has no `ExecutorKind`/`set_executor_kind`).

## Running the benches

```text
cargo bench --offline --locked -p vitality --bench bench_systems              # every group
cargo bench --offline --locked -p vitality --bench bench_systems -- scenario  # one group
```

Exactly one `--`: everything after it goes to criterion; a second one runs the whole suite. The filter is a regex against the full id, which carries its shape (`Workload::ALL`, `scenario_id` in `benches/support/scenario.rs`).

`VITALITY_SCENARIO_ACTORS=10000` sets the size; `--features multi_threaded` gives the `storm` figure. Check `/proc/loadavg` before and after; under ~10 % is directional. The gate lints the bench targets but never links or runs one — `cargo bench --no-run` is the link check.
