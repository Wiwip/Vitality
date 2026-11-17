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

From the `scenario` group of `crates/vitality/benches/bench_systems.rs`, measured 2026-11-17 on a shared 8-core box, load-gated: a run starts only under a 1-min loadavg of 4.0, and waits of 10-12 minutes were routine. Shape: 10 000 actors, 20 attributes and 3 effects each. The box was far noisier for most of that day than in the earlier sessions — inside that same gate `storm` spread 47 % and `idle` 57 % where they spread ~10 % before — so cross-tip claims rest only on paired same-session A/B runs, and a lone number says what the box was doing. A quiet window opened later (1-min load 1.65-3.70, no gate waits at all) and the 46 runs taken inside it spread 1.4-6.2 %: the `steady` and `storm` absolutes below come from those, with the noisy session quoted beside each so you can see why a single figure means nothing here; the `idle` figures were re-taken after that in gated paired runs at `6d9892e` (1-min load 3.0-3.8), quiet enough for a median and not for a 1 % claim.

- `steady`, 10 % of the actors' attributes moved, single-threaded: median **1.033 ms** over 11 runs in the quiet window at `70cbb3e`, 1.4 % spread — **inside the 2 ms ceiling**, and it agrees with the 1.0490 ms the same shape gave quietly at `25fa316`. The same shape read 1.2640 / 1.3067 / 1.5498 ms in the noisy session, which is the box and not the code. Id `scenario/steady/actors=10000/a=20/e=3/st-static`.
- `storm`, every attribute of every actor, with bevy's `multi_threaded`: median **6.14 ms** over 12 quiet runs, 3.6 % spread — **clears the 8 ms ceiling** by 23 %, against 6.5166 ms for the same shape at `25fa316`. The noisy session printed 6.2183 / 8.6345 / 9.1370 ms, two of three over the line, and that is why this claim was left open rather than reversed; the clean run settles it. The ceiling is a promise about the threaded build — single-threaded `storm` is 8.81 ms, over it. Id `scenario/storm/actors=10000/a=20/e=3/mt-static`.
- `idle`, nothing moved at all: median **152.42 µs** single-threaded against 255.6 µs on the paired pre-change build, 6/6 alternating pairs, 40 % off. The floor was never bevy's frame path: with only vitality's systems taken out of the schedules of the same 80 744-entity world a frame is 4.4 µs, and 248 µs of the old 254 sat inside `FixedUpdate` — chiefly `evaluate_effect_conditions` re-deciding the verdict of every condition-free effect, a walk that is gone now. The rungs re-taken since confirm it at the smaller shapes: 33.25 µs at 1 000 actors (44.40 before) and 86.61 µs at 5 000 (140.99 before). Multi-threaded `idle` is measured now and the 333.42 µs beside it was the noisy session: at `6d9892e` four gated pairs read 245.46 / 241.75 / 232.53 / 230.13 µs (median 237.14) against 147.80 / 144.55 / 160.43 / 144.98 µs single-threaded, so with nothing moving the threaded build is still the slower one. That is not the walk refusing to parallelise: splitting it across threads wins 13.5 % of the multi-threaded figure (4/4 pairs, 185-213 µs) and loses 61.5 % of the single-threaded one (4/4 pairs, 146 → 233 µs), which is why it is not landed. Without bevy's `multi_threaded` feature `par_iter_mut` compiles to a serial fold and the task pool still reports every core, so nothing at runtime tells the two builds apart, and the single-threaded cost is the parallel command buffer bevy walks each tick on behalf of the split — paid even when the walk queues nothing. Ids `scenario/idle/actors=10000/a=20/e=3/{st,mt}-static`.
- Multi-threading is worth **1.44x** on `storm` (8.81 ms st against 6.14 ms mt) and **1.24x** on `steady` (1.033 ms against 0.831 ms), re-taken at `70cbb3e` as 12 and 11 alternating st/mt pairs in one quiet window: the mt build is faster in every pair, the arms spread 5.0 %/3.6 % on `storm` and 1.4 %/6.2 % on `steady`, and the pair ratios span 1.41-1.48x on `storm`, so both numbers sit well clear of the ~10 % noise floor. The `7340cfb` pairing read 1.28x on `storm` and nothing on `steady` — its `steady` half was two pairs, too thin to have been called anything; `steady` separates now, mt ~19 % ahead. The only per-tick walk that runs on threads is the resolver's dirty-actor pass; the condition walk is the one that would parallelise and was not worth its single-threaded cost, and neither build parallelises the resolve node itself, which holds `&mut World` for the whole pass. Ids `scenario/{steady,storm}/actors=10000/a=20/e=3/{st,mt}-static`.

## Running the benches

```text
cargo bench --offline --locked -p vitality --bench bench_systems              # every group
cargo bench --offline --locked -p vitality --bench bench_systems -- scenario  # one group
```

Exactly one `--`: everything after it goes to criterion, and a second one turns a filtered run into the whole suite. Criterion matches that filter against the full id as a regex, and the id carries the shape it came from — `Workload::ALL` names the workload and `scenario_id` builds the id, both in `benches/support/scenario.rs`. `VITALITY_SCENARIO_ACTORS=10000` runs one size, `--features multi_threaded` is the `storm` figure, `--features dyn` only reproduces pre-2026-11-10 numbers, which were all `mt-dyn`. Read `/proc/loadavg` before and after the run and report anything under ~10 % as directional. The gate never builds bench targets, so `--no-run` is how you check that they compile.
