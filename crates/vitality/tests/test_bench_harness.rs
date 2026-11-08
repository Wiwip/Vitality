//! Guards the bench harness (`benches/support/common.rs`) before any bench trusts it: the tiny
//! spec must really put its attributes on every actor, really let a permanent `ModOp::Add` modifier
//! move `current_value` off `base_value`, and really re-resolve when a base value is written.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/leaf.rs"]
mod leaf;

use bevy::ecs::bundle::Bundle;
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
    let (pool, added) = {
        let world = fixture.app.world();
        let harness = world.resource::<HarnessWorld>();
        (
            harness.attrs(),
            permanent_add(harness.plan.as_slice(), leaf::LEAF_SLOT as usize),
        )
    };
    assert!(
        added > 0.0,
        "no permanent Add landed on the leaf attribute (added {added}), so this test cannot tell a \
         resolved current_value from a pending one"
    );
    assert!(
        !fixture.actors.is_empty(),
        "the tiny spec built no actor to read"
    );

    // Cloned because `four_paths` takes the fixture mutably; the names borrow it otherwise.
    let actors = fixture.actors.clone();
    for &actor in actors.iter() {
        let leaf_spec = &pool[leaf::LEAF_SLOT as usize];
        let (base, current, expected) = {
            let world = fixture.app.world();
            (
                leaf_spec.base(world, actor).unwrap(),
                leaf_spec.current(world, actor).unwrap(),
                current_values(world, pool, actor)[leaf::LEAF_SLOT as usize],
            )
        };
        assert_ne!(
            base, current,
            "{} on {actor:?} is unresolved (base == current == {base}): leaf_fixture must tick the \
             world before snapshotting, or all four paths agree on a pending value",
            leaf_spec.name
        );
        assert!(
            expected > 0.0,
            "the leaf attribute on {actor:?} reads {expected}: nothing for the paths to agree on"
        );

        for (path, got) in four_paths(&mut fixture, actor) {
            assert_eq!(
                got, expected,
                "{path} read {got}, harness says {expected} on {actor:?}"
            );
        }
    }

    // Green above is not enough: the tiny shape has 3 attributes and the benched shape 8, and the
    // modifier draw is over the attribute range, so the two worlds settle differently.
    benched_shape_agree();
}

/// The world `leaf/access_paths` and `leaf/warm_cold` actually time. Its leaf attribute draws no
/// permanent Add at `modifiers_per_effect = 1`, so `current_value` legitimately equals `base_value`
/// there and the settle check above is not available; what is checkable is that the resolver ran in
/// this world at all, and that the four paths still land on one value for every actor of it.
fn benched_shape_agree() {
    let spec = leaf::leaf_bench_spec();
    let mut fixture = leaf::leaf_fixture(&spec);
    assert!(
        !fixture.actors.is_empty(),
        "shape {} built no actor to read",
        spec.shape_id()
    );

    let probe = fixture.actor;
    let resolved: Vec<&str> = {
        let world = fixture.app.world();
        let pool = world.resource::<HarnessWorld>().attrs();
        pool.iter()
            .filter(|attr| attr.base(world, probe).unwrap() != attr.current(world, probe).unwrap())
            .map(|attr| attr.name)
            .collect()
    };
    assert!(
        !resolved.is_empty(),
        "shape {} resolved nothing on {probe}: every attribute still reads its base_value, so the \
         fixture is pending and every leaf number taken from it is a pending number",
        spec.shape_id()
    );

    let actors = fixture.actors.clone();
    for &actor in actors.iter() {
        let expected = {
            let world = fixture.app.world();
            let pool = world.resource::<HarnessWorld>().attrs();
            current_values(world, pool, actor)[leaf::LEAF_SLOT as usize]
        };
        for (path, got) in four_paths(&mut fixture, actor) {
            assert_eq!(
                got,
                expected,
                "{path} read {got}, harness says {expected} on {actor:?} of shape {}",
                spec.shape_id()
            );
        }
    }
}

/// The four `leaf/access_paths` reads for one actor, in id order. One list so a fifth path cannot be
/// added to the bench and to only one of the two guards above.
fn four_paths(fixture: &mut leaf::LeafFixture, actor: Entity) -> [(&'static str, f64); 4] {
    let world = fixture.app.world();
    [
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
    ]
}

/// `leaf/exclusion_length` (BENCH-PLAN §2C) puts its x-axis in the bench id, so two things have to
/// hold: every bundle width reads the same live value, and the width the id carries is the width
/// the read really scans. Runs on [`leaf::leaf_sweep_spec`], the shape the sweep times, not on
/// [`tiny_spec`]: the modifier draw depends on the attribute count, so settling the tiny shape says
/// nothing about the world the sweep's numbers come from.
#[test]
fn exclusion_length_reads_agree() {
    let spec = leaf::leaf_sweep_spec();
    let fixture = leaf::excl_fixture::<leaf::LeafBundle>(&spec);
    let world = fixture.app.world();
    let harness = world.resource::<HarnessWorld>();
    let added = permanent_add(harness.plan.as_slice(), leaf::LEAF_SLOT as usize);
    let pool = harness.attrs();
    assert!(
        added > 0.0,
        "shape {} lands no permanent Add on the leaf attribute (added {added}), so no tick can \
         move current_value off base_value here and this test cannot tell a settled read from a \
         pending one",
        spec.shape_id()
    );
    assert!(
        !fixture.actors.is_empty(),
        "shape {} built no actor to read",
        spec.shape_id()
    );

    for &actor in fixture.actors.iter() {
        let attr = &pool[leaf::LEAF_SLOT as usize];
        let base = attr.base(world, actor).unwrap();
        let current = attr.current(world, actor).unwrap();
        assert_ne!(
            base, current,
            "{} on {actor:?} is unresolved (base == current == {base}): excl_fixture must tick the \
             world before anything reads it, or every bundle width agrees on a pending value",
            attr.name
        );
    }

    width_agrees::<leaf::Excl0>(&spec, 0);
    width_agrees::<leaf::Excl5>(&spec, 5);
    width_agrees::<leaf::Excl20>(&spec, 20);
    width_agrees::<leaf::Excl40>(&spec, 40);
}

/// One width of the sweep: the id's two width numbers agree with each other and with what the
/// caller declares, and the read still lands on the live value. `Arity::ARITY` is the tuple's member
/// count, known without a `World`; `excl` is how many of those members this world has a
/// `ComponentId` for. A member that was never registered is invisible to `bundle_width` yet still
/// walked by `bundle_contains_component`, so `excl < arity` means the sweep's own axis is wrong.
fn width_agrees<B: Bundle + leaf::Arity>(spec: &WorldSpec, width: usize) {
    let name = std::any::type_name::<B>();
    let mut fixture = leaf::excl_fixture::<B>(spec);
    let world = fixture.app.world();
    let pool = world.resource::<HarnessWorld>().attrs();

    assert_eq!(
        B::ARITY,
        width,
        "{name} has {} members, not {width}: the sweep declares a width it does not build",
        B::ARITY
    );
    assert_eq!(
        fixture.excl, width,
        "{name}: this world registers {} of the {width} members the bundle declares. Build the \
         view before measuring the width — `QueryState::new` is what registers a marker component \
         the world has never seen, and an unregistered member is still scanned by the read, so the \
         bench would report a narrower bundle than it pays for",
        fixture.excl
    );

    for &actor in fixture.actors.iter() {
        let expected = current_values(world, pool, actor)[leaf::LEAF_SLOT as usize];
        let got = leaf::read_except(&leaf::leaf_view(world, &mut fixture.view, actor));
        assert_eq!(
            got, expected,
            "{name} read {got}, harness says {expected} on {actor:?}"
        );
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
