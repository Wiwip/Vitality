//! A live self-targeted modifier (`source == target == owner`) does not follow a base write to the
//! attribute it reads: what decides it is the write's cadence and the registration order, not density.

#[path = "../benches/support/common.rs"]
mod common;

use crate::common::{
    ATTRIBUTE_POOL, Attr0, Attr1, WorldSpec, actor_entities, build_app_with, tick,
};
use bevy::prelude::*;
use vitality::attributes::{Attribute, AttributeDependents};
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{AttributeModifier, EffectSubject, ModOp, OwnedModifiers};
use vitality::effect::{AppliedEffects, ApplyEffectEvent, EffectBuilder, EffectTargeting};

const WRITE: f64 = 100.0;

const SETTLE: usize = 3;

#[test]
fn a_self_buff_written_the_tick_after_it_lands_follows_the_source_move() {
    let spec = WorldSpec {
        attributes_per_actor: 2,
        effects_per_actor: 0,
        modifiers_per_effect: 1,
        actors: 1,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = build_app_with(&spec, ATTRIBUTE_POOL);
    let actor = actor_entities(app.world_mut())[0];
    let handle = app.world_mut().resource_mut::<Assets<EffectDef>>().add(
        EffectBuilder::permanent()
            .modify::<Attr1>(Attr0::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );

    // the apply cadence `scenario::queue_apply` uses: queued, so it lands inside the next tick
    {
        let world = app.world_mut();
        let mut commands = world.commands();
        commands.trigger(ApplyEffectEvent {
            entity: actor,
            targeting: EffectTargeting::new(actor, actor),
            handle,
        });
    }
    app.world_mut().flush();
    tick(&mut app);

    let (effect, modifier) = {
        let world = app.world();
        let effects: Vec<Entity> = world.get::<AppliedEffects>(actor).unwrap().iter().collect();
        let effect = *effects.last().expect("the self-buff landed on the actor");
        let modifiers: Vec<Entity> = world
            .get::<OwnedModifiers>(effect)
            .unwrap()
            .iter()
            .collect();
        (effect, modifiers[0])
    };
    let source = ATTRIBUTE_POOL[0].current(app.world(), actor).unwrap() as f32;
    let before = ATTRIBUTE_POOL[1].current(app.world(), actor).unwrap() as f32;
    let base = ATTRIBUTE_POOL[1].base(app.world(), actor).unwrap() as f32;
    assert!(
        app.world()
            .get::<AttributeDependents<Attr0>>(actor)
            .is_some(),
        "the dependency edge has to be on the actor or this test is not about the notify step"
    );
    assert!(
        (before - (base + source)).abs() < 1e-3,
        "the apply is not folded yet: Attr1 {before}, want base {base} + Attr0 {source}"
    );

    ATTRIBUTE_POOL[0].set_base(app.world_mut(), actor, source as f64 + WRITE);
    for _ in 0..SETTLE {
        tick(&mut app);
    }

    // the cascade ran: the modifier knows the new value, so the loss is the refresh after it
    let value = app
        .world()
        .get::<AttributeModifier<Attr1>>(modifier)
        .unwrap()
        .value;
    assert!(
        (value - (source + WRITE as f32)).abs() < 1e-3,
        "the dependent modifier was not even re-evaluated: {value}, want {}",
        source + WRITE as f32
    );
    let after = ATTRIBUTE_POOL[1].current(app.world(), actor).unwrap() as f32;
    assert!(
        (after - (before + WRITE as f32)).abs() < 1e-3,
        "self-buff on {actor:?} (effect {effect:?}): Attr0 moved +{WRITE} and the modifier followed, \
         but Attr1 stayed at {after}, want {}",
        before + WRITE as f32
    );
}
