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

## Performance

`BENCH-RESULTS.md` has the numbers. The short version: at 10 000 actors with 20 attributes and 3
effects each, moving 10 % of the actors' attributes costs about 1.2 ms a tick single-threaded,
and moving every attribute of every actor costs about 6 ms with bevy's `multi_threaded` feature.
