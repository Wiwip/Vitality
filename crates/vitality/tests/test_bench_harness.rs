//! Guards the bench harness (`benches/support/common.rs`) before any bench trusts it: the tiny
//! spec must really put its attributes on every actor, really let a permanent `ModOp::Add` modifier
//! move `current_value` off `base_value`, and really re-resolve when a base value is written.

#[path = "../benches/support/common.rs"]
mod common;

use bevy::prelude::*;
use common::{EffectPlan, HarnessWorld, WorldSpec, actor_entities, tick};

fn tiny_spec() -> WorldSpec {
    WorldSpec {
        attributes_per_actor: 3,
        effects_per_actor: 2,
        modifiers_per_effect: 1,
        actors: 2,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// Total permanent `Add` amount landing on one attribute index.
fn permanent_add(plan: &[EffectPlan], attribute: usize) -> f64 {
    plan.iter()
        .filter(|effect| !effect.ticking)
        .flat_map(|effect| effect.modifiers.iter())
        .filter(|modifier| modifier.attribute == attribute)
        .map(|modifier| modifier.amount as f64)
        .sum()
}

#[test]
fn tiny_spec_drives_the_resolver() {
    let mut app = tiny_spec().build_app();
    let pool = app.world().resource::<HarnessWorld>().attrs();

    let actors = actor_entities(app.world_mut());
    assert_eq!(actors.len(), tiny_spec().actors, "one entity per spec.actors");
    for &actor in actors.iter() {
        for attr in pool {
            assert!(
                attr.current(app.world(), actor).is_some(),
                "{} missing on actor {actor:?}",
                attr.name
            );
        }
    }

    let added: Vec<f64> = {
        let plan = app.world().resource::<HarnessWorld>().plan.as_slice();
        assert!(
            plan.iter()
                .any(|effect| !effect.ticking && !effect.modifiers.is_empty()),
            "the tiny spec produced no permanent modifier: {plan:?}"
        );
        (0..pool.len())
            .map(|index| permanent_add(plan, index))
            .collect()
    };
    assert!(
        added.iter().sum::<f64>() > 0.0,
        "no permanent Add modifier landed on an attribute: {added:?}"
    );

    tick(&mut app);

    for &actor in actors.iter() {
        for (index, attr) in pool.iter().enumerate() {
            let base = attr.base(app.world(), actor).unwrap();
            let current = attr.current(app.world(), actor).unwrap();
            let expected = base + added[index];
            assert!(
                (current - expected).abs() < 1e-3,
                "{}: current {current} != base {base} + add {} on {actor:?}",
                attr.name,
                added[index],
            );
            if added[index] > 0.0 {
                assert_ne!(
                    current, base,
                    "a permanent Add on {} left current_value == base_value",
                    attr.name
                );
            }
        }
    }

    let actor = actors[0];
    let base_before = pool[0].base(app.world(), actor).unwrap();
    let current_before = pool[0].current(app.world(), actor).unwrap();

    pool[0].set_base(app.world_mut(), actor, base_before + 5.0);
    tick(&mut app);

    let base_after = pool[0].base(app.world(), actor).unwrap();
    let current_after = pool[0].current(app.world(), actor).unwrap();
    assert!(
        (base_after - (base_before + 5.0)).abs() < 1e-3,
        "base write lost: {base_before} -> {base_after}"
    );
    assert!(
        (current_after - (current_before + 5.0)).abs() < 1e-3,
        "the tick after a base write left current_value at {current_after}, expected {}",
        current_before + 5.0
    );
}
