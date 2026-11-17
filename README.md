# vitality

Gameplay attributes, effects and abilities for Bevy.

## Crates
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

- Attributes are components: read them with `Query<&Health>`, react with `Changed<Health>` or
  `On<CurrentValueChanged<Health>>`.
- `Effects` applies, finds and removes effects (by entity, definition, tag or source) and edits
  stacks. Lifecycle events: `EffectApplied`, `EffectRemoved` (with a reason), `EffectStacksChanged`,
  `EffectBlocked` — fired only by the tag rules (a missing required tag, or a blocked-by tag on the
  target); a failed `attach_if` or `active_while` fires no event at all. `attach_if` is read once,
  when the effect is applied, and only by the effects that spawn an entity: a miss simply never
  applies. `active_while` is the one that goes `EffectInactive` on a later tick.
- `Abilities` grants, activates (`try_activate`, `can_activate`), cancels and revokes abilities.
  Events: `AbilityActivated`, `AbilityActivationFailed` (with an `ActivationError`), `AbilityEnded`.
- Tags are components granted with a count (`Vitality::grant_tag`, effect `with_tag`), so two
  sources holding the same tag keep it until both let go. Effects can require, be blocked by, or
  cleanse tags; abilities can be blocked by, block, or cancel by tag.
- Modifiers that read another attribute are live by default and settle in the same tick, across
  actors too. `modify_snapshot` evaluates once at application instead.

The attribute pipeline (the `EffectsSet` chain) runs on `FixedUpdate`, so a headless app needs a
`TimeUpdateStrategy` that advances time. Ability timers stay on the frame clock:
`tick_ability_cooldown` runs in `Update`, `handles_wait_task_timers` in `PreUpdate`.

## Wiring your own plugin

The contract, with what each break looks like from inside an app, is the `vitality` crate-level doc,
i.e. the crate's own rustdoc page (`crates/vitality/src/lib.rs`). Not `vitality::AttributesPlugin`:
that page is a three-line pointer to it and holds no contract. Short version:

- Register attributes with `vitality::init_attribute::<T>()` in the plugin's `build`. `Plugin::finish`
  only runs when the app runner calls `App::finish()`, which `App::update()` never does, and an
  attribute nobody registered silently applies no modifiers and fires no value-changed events.
- `AttributesPlugin` comes before anything that calls `init_attribute`; it owns the registry the
  attribute is written into (that one panics, so it is the easy one).
- The 128-attribute cap is one counter for the whole app, not one per crate, and vitality's own
  `Stacks` attribute uses a slot. The 129th registration names itself and panics.
- `AttrId` is a registration index, so it moves when plugin order moves. Persist the attribute's
  name, never the id — and keep names unique across crates, because the name is the type's short
  name and a duplicate panics at build.
- Add `EffectsPlugin`. The DAG's apply-path nodes (`resolve_system`, `apply_all_modifier_events`,
  `scan_base_writes`, `apply_all_periodic_effects`) are installed once by `EffectsPlugin::build` and
  `init_effect_attribute` installs none of them, so an app that skips `EffectsPlugin` never applies
  anything at all — nothing listens for the apply event. Inserting anything once the plugins are all
  built changes nothing: those nodes, and the per-attribute state `init_effect_attribute` adds, are
  all added at build.

## Performance

The numbers come from the `scenario` group of `crates/vitality/benches/bench_systems.rs`, which
measures them rather than storing them. The short version, from the 2026-11-16 run at tip `7340cfb`
on a shared 8-core box (load 2.5-5.1, absolute values move ~10 % run to run): at 10 000 actors with
20 attributes and 3 effects each, moving 10 % of the actors' attributes — the `steady` workload —
costs roughly 1.1 ms a tick single-threaded (1.1325 / 1.1479 / 1.1436 ms over three runs, against the
2 ms target), and moving every attribute of every actor — `storm` — roughly 7 ms with bevy's
`multi_threaded` feature (mean 7.19 ms, worst run 7.39 ms, worst upper bound 7.73 ms against the 8 ms
target, so it is met with nothing to spare). Multi-threading is worth 1.28x on `storm`, nothing on
`steady`, and it loses on the `idle` control. Nothing was benched after that run, so the claim for the
current tip rests on the dag tick's code path being unchanged since `7340cfb`. Their criterion ids are
`scenario/steady/actors=10000/a=20/e=3/st-static` and
`scenario/storm/actors=10000/a=20/e=3/mt-static`: `Workload::ALL` names the workloads and
`scenario_id` builds that id, both in `crates/vitality/benches/support/scenario.rs`, and criterion
matches a trailing filter against the full id as a regex, so a filtered run is
`cargo bench --offline --locked -p vitality --bench bench_systems -- scenario`, with
`VITALITY_SCENARIO_ACTORS=10000` for one size and `--features multi_threaded` for the `storm` figure.
