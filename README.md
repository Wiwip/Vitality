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

Tested feature matrix: `--features dyn`, `--features multi_threaded`, `--features dyn,multi_threaded` and `--no-default-features` all pass `cargo test --offline --locked --workspace --lib --tests` on their own (`-p vitality`) and as `--workspace`; `--doc` is green in every combination but `dyn`, whose `dynamic_linking` doctest binaries cannot load `libstd.so` (D-22).

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

Attributes are components: read them with `Query<&Health>`, react with `Changed<Health>` or `On<CurrentValueChanged<Health>>`. `Effects` applies, finds and removes effects (by entity, definition, tag or source) and edits stacks, `Abilities` grants, activates, cancels and revokes, and their events follow the change: `EffectApplied`, `EffectRemoved` (with a reason), `EffectStacksChanged`, `AbilityActivated`, `AbilityActivationFailed` (with an `ActivationError`), `AbilityEnded`. `EffectBlocked` fires only on a tag rule — a failed `attach_if` or `active_while` fires no event at all.

The attribute pipeline (the `EffectsSet` chain) runs on `FixedUpdate`, so a headless app needs a `TimeUpdateStrategy` that advances time; ability timers stay on the frame clock (`tick_ability_cooldown` in `Update`, `handles_wait_task_timers` in `PreUpdate`).

## Wiring your own plugin

The contract, with what each break looks like from inside an app and the test that pins it, is the `vitality` crate-level doc:

- Register attributes with `vitality::init_attribute::<T>()` in `Plugin::build`: `App::update()` never calls `App::finish`, so a registration made in `Plugin::finish` never runs and that attribute silently applies no modifiers and fires no value-changed events.
- `AttributesPlugin` owns the registry the attribute is written into, so it comes before anything that calls `init_attribute`; that one panics at build.
- The 128-attribute cap is one counter for the whole app, not one per crate, and vitality's own `Stacks` attribute uses a slot, so an app gets 127 game attributes and the 128th is the one refused, by name.
- `AttrId` is a registration index, so it moves when plugin order moves. Persist the attribute's name, never the id — and keep names unique across crates, because the name is the type's short name and a duplicate panics at build.
- Add `EffectsPlugin`. The DAG's apply-path nodes (`resolve_system`, `apply_all_modifier_events`, `scan_base_writes`, `apply_all_periodic_effects`) are installed once by `EffectsPlugin::build` and `init_effect_attribute` installs none of them, so an app that skips `EffectsPlugin` never applies anything: nothing listens for the apply event. Nodes and per-attribute state are all added at build, so inserting later changes nothing.

## Performance

From the `scenario` group of `crates/vitality/benches/bench_systems.rs`, measured 2026-11-17 on a shared 8-core box (start-of-run loadavg 3.1-3.9 on every quoted run; absolute values move ~10 % run to run, so the pair under each ceiling number is the previous build run alternately in the same session). Shape: 10 000 actors, 20 attributes and 3 effects each.

- `steady`, 10 % of the actors' attributes moved, single-threaded: 1.0463 / 1.0479 / 1.0501 / 1.0527 ms over four runs, median 1.05 ms, 0.6 % spread — **inside the 2 ms ceiling** with 47 % to spare. The paired runs, same session: 1.1445 / 1.1517 / 1.1729 / 1.2396 ms, median 1.16 ms; 4/4 pairs and 9.8 % on the medians. Id `scenario/steady/actors=10000/a=20/e=3/st-static`.
- `storm`, every attribute of every actor, with bevy's `multi_threaded`: median 6.52 ms over six alternating pairs (6.1753-6.9185 ms, 12 % spread) against 7.07 ms on the paired build (6.7606-7.3073 ms); 6/6 pairs and 7.9 % on the medians. **Inside the 8 ms ceiling** with room now: the slowest run of the six (6.9185 ms, upper bound 6.9805 ms) leaves 12.7 %, where `5866cd9` left 5 %. Id `scenario/storm/actors=10000/a=20/e=3/mt-static`.
- `idle`, nothing moved at all: 253.97 / 254.32 / 254.77 µs single-threaded and 337.17 / 349.56 / 387.82 µs with `multi_threaded`, so an empty tick costs 0.25 ms and multi-threading makes it 1.37x **worse**. A scratch probe build put ~1.6 µs of that inside vitality's `FixedUpdate` content, so the floor is bevy's own frame path, and it still scales with actor count: 44 µs at 1 000, 141 µs at 5 000, 254 µs at 10 000. Ids `scenario/idle/actors=10000/a=20/e=3/{st,mt}-static`.
- Multi-threading is worth 1.28x on `storm` and nothing on `steady`. That pairing was taken at `7340cfb` and has not been re-taken since; the `idle` half of it has, at this tip.

## Running the benches

```text
cargo bench --offline --locked -p vitality --bench bench_systems              # every group
cargo bench --offline --locked -p vitality --bench bench_systems -- scenario  # one group
```

Exactly one `--`: everything after it goes to criterion, and a second one turns a filtered run into the whole suite. Criterion matches that filter against the full id as a regex, and the id carries the shape it came from — `Workload::ALL` names the workload and `scenario_id` builds the id, both in `benches/support/scenario.rs`. `VITALITY_SCENARIO_ACTORS=10000` runs one size, `--features multi_threaded` is the `storm` figure, `--features dyn` only reproduces pre-2026-11-10 numbers, which were all `mt-dyn`. Read `/proc/loadavg` before and after the run and report anything under ~10 % as directional. The gate never builds bench targets, so `--no-run` is how you check that they compile.
