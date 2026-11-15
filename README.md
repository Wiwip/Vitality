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

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            VitalityPlugin::new()
                .attribute::<Health>()
                .attribute::<MaxHealth>()
                .attribute::<Power>(),
        )
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut vitality: Vitality) {
    let hero = vitality
        .add_spawn_actor(
            ActorBuilder::new()
                .name("Hero")
                .with::<Health>(100.0)
                .with::<MaxHealth>(100.0)
                .with::<Power>(10.0)
                .clamp::<Health>(0.0, MaxHealth::src())
                .build(),
        )
        .id();

    // a 5 s burn that scales with the caster's Power and tags the target while it lasts
    let burn = vitality.add_effect(
        EffectBuilder::every_second_for_duration(1.0, 5.0)
            .with_tag::<Burning>()
            .modify::<Health>(Power::src(), ModOp::Sub, EffectSubject::Target)
            .build(),
    );
    vitality.apply_effect_to_self(hero, &burn);
}
```

- Attributes are components: read them with `Query<&Health>`, react with `Changed<Health>` or
  `On<CurrentValueChanged<Health>>`.
- `Effects` applies, finds and removes effects (by entity, definition, tag or source) and edits
  stacks. Lifecycle events: `EffectApplied`, `EffectRemoved` (with a reason), `EffectStacksChanged`,
  `EffectBlocked`.
- `Abilities` grants, activates (`try_activate`, `can_activate`), cancels and revokes abilities.
  Events: `AbilityActivated`, `AbilityActivationFailed` (with an `ActivationError`), `AbilityEnded`.
- Tags are components granted with a count (`Vitality::grant_tag`, effect `with_tag`), so two
  sources holding the same tag keep it until both let go. Effects can require, be blocked by, or
  cleanse tags; abilities can be blocked by, block, or cancel by tag.
- Modifiers that read another attribute are live by default and settle in the same tick, across
  actors too. `modify_snapshot` evaluates once at application instead.

Everything runs on `FixedUpdate`. A headless app needs a `TimeUpdateStrategy` that advances time.

## Wiring your own plugin

The contract, with what each break looks like from inside an app, is the `vitality` crate-level doc,
i.e. the crate's own rustdoc page (`crates/vitality/src/lib.rs`). Not `vitality::AttributesPlugin`:
that page is a one-liner and holds no contract. Short version:

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
- Set the resolver mode before the plugins are added (`VitalityPlugin::new().resolver(mode)`), and
  add `EffectsPlugin`. The DAG's apply-path nodes (`resolve_system`, `apply_all_modifier_events`,
  `scan_base_writes`, `apply_all_periodic_effects`) are installed once by `EffectsPlugin::build` and
  only for the mode it sees there; `init_effect_attribute` installs none in DAG mode. So an app that
  skips `EffectsPlugin` never applies anything at all — nothing listens for the apply event — and one
  that runs `EffectsPlugin::build` under `ResolverMode::PerAttribute` while its attributes register
  under `ResolverMode::Dag` (add `AttributesPlugin` under `PerAttribute`, insert `ResolverMode::Dag`,
  then call `init_attribute`) applies no modifier and never fires a periodic effect, even though the
  effect still reports itself applied. The reverse order is not that case: `Dag` at
  `EffectsPlugin::build` with the attributes registered under `PerAttribute` runs as a mixed-mode
  app — vitality's own attributes keep the DAG nodes, the later ones take the per-attribute path —
  and applies and fires like a plain per-attribute app. It pays both paths though: the later
  attributes are also picked up by the mode-blind `scan_base_writes`, which walks every registered
  attribute each tick, by `resolve_system` whenever anything is queued, and by the global
  `mark_actor_dirty_dag` observer — and nothing orders `resolve_system` against
  `update_current_value_system::<T>`. That mixed mode runs, but no test pins it.
  A mode inserted after the plugins are built changes nothing: all three reads of it are at build.
  Whether that stays a contract or becomes a first-install guard inside
  `init_effect_attribute` is open (D-52).

## Performance

The numbers come from the `scenario` group of `crates/vitality/benches/bench_systems.rs`, which
measures them rather than storing them. The short version, from the 2026-11-10/11 runs on a shared
24-core box where absolute values move ~10 % run to run: at 10 000 actors with 20 attributes and 3
effects each, moving 10 % of the actors' attributes — the `steady` workload — cost roughly 1.2 ms a
tick single-threaded, and moving every attribute of every actor — `storm` — roughly 6 ms with bevy's
`multi_threaded` feature. That `storm` figure has not been retaken since the per-attribute
`FixedUpdate` nodes were collapsed. Both are DAG-resolver ticks, so their criterion ids are
`scenario/steady/actors=10000/a=20/e=3/dag/st-static` and
`scenario/storm/actors=10000/a=20/e=3/dag/mt-static`: `Workload::ALL` names the workloads and
`scenario_id` builds that id, both in `crates/vitality/benches/support/scenario.rs`, and criterion
matches a trailing filter against the full id as a regex, so
`VITALITY_SCENARIOS_ONLY=1 VITALITY_RESOLVER=dag VITALITY_SCENARIO_ACTORS=10000 cargo bench
--offline --locked -p vitality --bench bench_systems -- 'scenario/(steady|storm)/'` is how they are
taken — add `--features multi_threaded` for the `storm` figure.
