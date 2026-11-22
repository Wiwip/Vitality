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

Attributes are components: read them with `Query<&Health>`, react with `Changed<Health>` or `On<CurrentValueChanged<Health>>`. `Effects` applies, finds and removes effects by entity, definition, tag or source and edits stacks; `Abilities` grants, activates, cancels and revokes.

The events that follow the change: `EffectApplied`, `EffectRemoved` (with a reason), `EffectStacksChanged`, `AbilityActivated`, `AbilityActivationFailed` (with an `ActivationError`), `AbilityEnded`. `EffectBlocked` fires only on a tag rule — a failed `attach_if` or `active_while` fires no event at all.

The attribute pipeline (the `EffectsSet` chain) runs on `FixedUpdate`, so a headless app needs a `TimeUpdateStrategy` that advances time; ability timers stay on the frame clock — `handles_wait_task_timers` runs in `PreUpdate`, and the cooldown is a `StateTimer` entry you read with `cooldown_remaining`.

## Def files

The declarative half of an effect or an ability: `effect_def_from_file` and `ability_def_from_file` read a def off disk, `effect_def_from_str` and `ability_def_from_str` the same text out of a bundle. Every line is `key = value`, `#` starts a comment and runs to the end of its line, and a key may repeat where the def holds a list.

```text
name = Regeneration
policy = every 1.0 for 12.0
modifier = Health add 2 target
```

An effect file holds `name`, `policy`, `tag`, `requires`, `blocked_by`, `cleanses` and `modifier`, and no `policy` means a permanent effect.

`policy` is `instant`, `permanent`, `for <seconds>`, `every <seconds>` or `every <seconds> for <seconds>`; `modifier` is `<attribute> <op> <value> <who>`, the op and the who spelled as the `ModOp` and `EffectSubject` variant names.

An ability file holds `name`, `cooldown`, `tag`, `activation_owned`, `activation_requires`, `activation_blocked_by`, `blocks` and `cancels`.

A file can only name what the app put in `DefNames`: `init_attribute::<T>()` adds the attribute, you add a tag yourself with `DefNames::tag::<T>()`, and both are spelled as the short type name.

Anything else fails its line — ``line 2: no tag `Frost`: a def file only names tags the app put in DefNames`` — and `crates/vitality/tests/test_def_file_load.rs` pins the loads and those errors.

Costs, expressions, clamps, task scenes and observers stay Rust: an ability's cost, a clamp or a `Power::src()` read cannot come out of a file.

## Your own number type

An attribute folds in its own property type: `(base + Σadd − Σsub) × (1 + Σincrease) × Πmul`, one operation at a time, a stacked `Mul` as `powi(stacks)`, so a type that rounds differently from `f64` or runs past it gets its own arithmetic back. Any primitive number works as is.

A host type implements `AttributeValue` (`zero`, `one`, `add`, `sub`, `mul`, `powi`, `from_f64`, `to_f64`), derives `Reflect`, and calls `property_number!(MyType)` so its values can be expression literals.

Then `attribute!(Gold, MyType)` and `EffectBuilder::modify::<Gold>(MyType::..., ..)` take it directly, and literals, attribute reads and clamps stay in the type.

Two limits: expression arithmetic (`A::src() + B::src()`) comes from express-it and needs `num_traits::Num`, and def files and the debug overlay read through `to_f64`, so those are lossy past `f64`. `crates/vitality/tests/test_property_types.rs` is a worked example.

## Wiring your own plugin

The contract, with what each break looks like from inside an app and the test that pins it, is the `vitality` crate-level doc:

- Register attributes with `vitality::init_attribute::<T>()` in `Plugin::build`: `App::update()` never calls `App::finish`, so a registration made in `Plugin::finish` never runs and that attribute fires no value-changed events — a permanent effect on it silently applies no modifiers, an instant one drops its queued write with a warn naming the attribute.
- `AttributesPlugin` owns the registry the attribute is written into, so it comes before anything that calls `init_attribute`; that one panics at build.
- The 128-attribute cap is one counter for the whole app, not one per crate, and vitality's own `Stacks` attribute uses a slot, so an app gets 127 game attributes and the 128th is the one refused, by name.
- `AttrId` is a registration index, so it moves when plugin order moves. Persist the attribute's name, never the id — and keep names unique across crates, because the name is the type's short name and a duplicate panics at build.
- Add `EffectsPlugin`: its `build` installs the apply-path nodes (`resolve_system`, `apply_all_modifier_events`, `scan_base_writes`, `apply_all_periodic_effects`) and `init_effect_attribute` installs none, so an app that skips it never applies anything — nothing listens for the apply event. Adding it after build changes nothing.

## Performance

From the `scenario` group of `crates/vitality/benches/bench_systems.rs` at `1f0c562`, 2026-11-22, on this box (8-CPU quota, 1-min loadavg 1.2-2.8 across the runs, no cgroup throttling). Shape: 10 000 actors, 20 attributes and 3 effects each; each figure is the median of three runs of the filtered group, one build single-threaded and one with `--features multi_threaded`.

- `steady`, 10 % of the actors' attributes moved: **1.338 ms** single-threaded, 4.6 % spread — **inside the 2 ms ceiling**. Multi-threaded it is 6.721 ms. Ids `scenario/steady/actors=10000/a=20/e=3/{st,mt}-static`.
- `storm`, every attribute of every actor: **12.27 ms** single-threaded, 10 % spread, and **33.7 ms** multi-threaded — over the 8 ms ceiling, and directional, since the three runs read 21.6 / 33.7 / 33.9 ms. Ids `scenario/storm/actors=10000/a=20/e=3/{st,mt}-static`.
- `idle`, nothing moved at all: **288.4 µs** single-threaded, 3.7 % spread, and **2.279 ms** multi-threaded. Ids `scenario/idle/actors=10000/a=20/e=3/{st,mt}-static`.
- Multi-threading still costs ticks rather than saving them on all three shapes — 7.9x on `idle`, 5.0x on `steady`, 2.7x on `storm` (directional) — but the fan-out is the cost, not the work under it. The condition walk is serial in both builds, and pinning the threaded process to one core reads 337.5 µs on `idle` against 271.9 µs for the single-threaded build on that same core: four fifths of the threaded `idle` tick is the executor and the schedule — deleting the parallel condition walk dropped threaded `idle` from 3.15 ms to 2.28 ms and left the multiplier at 7.9x. The rest of that tick is hand-off too, and none of it is a walk: at `idle` the 30 000 effect entities carry no `EffectDuration` and no `EffectTicker`, and `resolve_attributes` returns before the resolver walk on an empty queue, so nothing fans out — making the two timer walks serial read 2.18 ms against the 2.22 ms baseline, inside the run-to-run spread — and what is left is the schedule handing its sets to the 8-thread pool, since single-threaded executors on the four `*Update` schedules read 1.20 ms against 2.22 ms unpinned, 0.65 ms against 1.39 ms pinned to `0-7`, 0.75 ms against 1.57 ms on `0-3` and 0.30 ms against 0.34 ms on one core (means of 2 000 ticks of the `scenario` harness, not criterion figures). Ids `scenario/{idle,steady,storm}/actors=10000/a=20/e=3/{st,mt}-static`.

## Running the benches

```text
cargo bench --offline --locked -p vitality --bench bench_systems              # every group
cargo bench --offline --locked -p vitality --bench bench_systems -- scenario  # one group
```

Exactly one `--`: everything after it goes to criterion, and a second one turns a filtered run into the whole suite. Criterion matches that filter as a regex against the full id, and the id carries the shape it came from — `Workload::ALL` names the workload and `scenario_id` builds the id, both in `benches/support/scenario.rs`.

`VITALITY_SCENARIO_ACTORS=10000` runs one size and `--features multi_threaded` is the `storm` figure. Read `/proc/loadavg` before and after the run and report anything under ~10 % as directional. The gate never builds bench targets, so `--no-run` is how you check that they compile.
