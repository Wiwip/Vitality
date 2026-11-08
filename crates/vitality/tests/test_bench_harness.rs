//! Guards the bench harness (`benches/support/common.rs`) before any bench trusts it: the tiny
//! spec must really put its attributes on every actor, really let a permanent `ModOp::Add` modifier
//! move `current_value` off `base_value`, and really re-resolve when a base value is written.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/leaf.rs"]
mod leaf;

use bevy::prelude::*;
use common::{EffectPlan, HarnessWorld, WorldSpec, actor_entities, current_values, tick};

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

/// The four `leaf/access_paths` reads have to land on one number, or the bench compares four
/// different things (BENCH-PLAN §2C). It goes through the bench's own fixture, so a path that has
/// drifted from the live attribute value — a stale slot block, a bundle that excludes the
/// attribute, a `ComponentId` for the wrong type, a `current_value` read where `Attribute::at_base()`
/// belongs — fails here instead of in a report.
#[test]
fn all_access_paths_agree() {
    // Referenced so the compile-time pin inside `leaf.rs` is checked under this test's name.
    let _pin = leaf::pin_leaf_bundle_to_effects_view;

    let mut fixture = leaf::leaf_fixture(&tiny_spec());
    let world = fixture.app.world();
    let harness = world.resource::<HarnessWorld>();
    let pool = harness.attrs();
    let added = permanent_add(harness.plan.as_slice(), leaf::LEAF_SLOT as usize);
    assert!(
        added > 0.0,
        "no permanent Add landed on the leaf attribute (added {added}), so this test cannot tell a \
         resolved current_value from a pending one"
    );
    assert!(
        !fixture.actors.is_empty(),
        "the tiny spec built no actor to read"
    );

    for &actor in fixture.actors.iter() {
        let leaf_spec = &pool[leaf::LEAF_SLOT as usize];
        let base = leaf_spec.base(world, actor).unwrap();
        let current = leaf_spec.current(world, actor).unwrap();
        assert_ne!(
            base, current,
            "{} on {actor:?} is unresolved (base == current == {base}): leaf_fixture must tick the \
             world before snapshotting, or all four paths agree on a pending value",
            leaf_spec.name
        );

        let expected = current_values(world, pool, actor)[leaf::LEAF_SLOT as usize];
        assert!(
            expected > 0.0,
            "the leaf attribute on {actor:?} reads {expected}: nothing for the paths to agree on"
        );

        let paths: [(&str, f64); 4] = [
            (
                "(i) EntityRefExcept::get",
                leaf::read_except(&leaf::leaf_view(world, &mut fixture.view, actor)),
            ),
            ("(ii) EntityRef::get", leaf::read_entity_ref(world, actor)),
            (
                "(iii) EntityRef::get_by_id",
                leaf::read_by_id(world, actor, fixture.id),
            ),
            ("(iv) slot block", leaf::read_slot(world, actor)),
        ];
        for (path, got) in paths {
            assert_eq!(
                got, expected,
                "{path} read {got}, harness says {expected} on {actor:?}"
            );
        }
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
