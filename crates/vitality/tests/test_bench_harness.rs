//! Guards the bench harness (`benches/support/common.rs`) before any bench trusts it: the tiny
//! spec must really put its attributes on every actor, really let a permanent `ModOp::Add` modifier
//! move `current_value` off `base_value`, and really re-resolve when a base value is written.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/floor.rs"]
mod floor;
#[path = "../benches/support/leaf.rs"]
mod leaf;
#[path = "../benches/support/recompute.rs"]
mod recompute;

use bevy::ecs::bundle::Bundle;
use bevy::prelude::*;
use common::{
    AttrSpec, EffectPlan, HarnessWorld, WorldSpec, actor_entities, build_app_with, current_values,
    run_fixed_tick, tick,
};

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

/// Family A (`floor/idle_tick`, BENCH-PLAN §2A) times ticks that do nothing, so the shape has to be
/// provably idle before its ns/tick means anything: the actor count the spec asked for, every
/// attribute of *the pool that shape registered* on every actor, nothing left dirty anywhere in the
/// world, and no effect for a tick to resolve. The tiny version of the shape (3 attributes, 5
/// actors) lives here; the bench's 1000-actor shapes are the same call with bigger numbers.
///
/// The pool-length check is the point of the second assertion: a shape that registers a deeper pool
/// than it declares still gives every actor every attribute, so only `attrs().len()` catches that a
/// 1-attribute point is really paying for 80 registrations and would put a lie in the fitted slope.
///
/// What `HarnessWorld::plan` can prove here is narrower than the other three: it records what the
/// *generator* (`prepare_world`) built, not what a hand-built world contains. An empty `plan` says
/// this shape asked for no effects; it says nothing about an effect someone inserted by hand.
#[test]
fn floor_shape_builds() {
    let spec = floor::floor_spec(3, 5);
    let shape = floor::floor_shape(&spec, common::ATTRIBUTE_POOL);
    let world = shape.app.world();

    assert_eq!(
        shape.actors.len(),
        spec.actors,
        "the floor shape built {} actors, not the {} its id claims",
        shape.actors.len(),
        spec.actors
    );

    let pool = world.resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        spec.attributes_per_actor,
        "the shape asked for {} attributes and registered {}: a deeper pool than the shape declares \
         is what makes the floor's slope wrong, because every extra attribute brings its own \
         systems into every tick",
        spec.attributes_per_actor,
        pool.len()
    );
    for &actor in shape.actors.iter() {
        for attr in pool {
            assert!(
                attr.current(world, actor).is_some(),
                "{} missing on actor {actor:?}: a floor point whose actors do not carry the whole \
                 pool does not price the schedule it is labelled as",
                attr.name
            );
        }
    }

    // Positive control for the walk below: `build_app_with` returns after Startup only, so the
    // `Insert<T>` observers have dirtied every attribute of every actor and nothing has resolved.
    // A walk that could not see this would also report 0 after the prepare tick, and the floor would
    // be measured on a world nobody proved was clean.
    assert!(
        shape.dirty_at_build > 0,
        "the world was already clean when build_app_with returned ({} dirty entities found by the \
         walk): either the resolver ran during the build, or `dirty_entity_count` cannot see a \
         Dirty<T> at all and its 0 below proves nothing",
        shape.dirty_at_build
    );

    assert_eq!(
        floor::dirty_entity_count(world),
        0,
        "a prepared floor shape still has {} dirty entities: the timed ticks would be resolving, \
         not idling",
        floor::dirty_entity_count(world)
    );

    assert!(
        world.resource::<HarnessWorld>().plan.is_empty(),
        "the floor shape built {} effects: with a modifier to apply a tick is not idle",
        world.resource::<HarnessWorld>().plan.len()
    );
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
    assert_eq!(
        actors.len(),
        tiny_spec().actors,
        "one entity per spec.actors"
    );
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

/// The shape `test_recompute_ratio` settles: 3 attributes on 1 actor and 20 permanent effects x 1
/// modifier — the top row of the `recompute/*` grid, with A=3 rather than a benched 1/5/20 because 3
/// is the smallest A that has an attribute the tick is supposed to leave alone *and* a second one to
/// tell it apart from.
const RECOMPUTE_ATTRIBUTES: usize = 3;
const RECOMPUTE_EFFECTS: usize = 20;

/// Base write big enough to survive `f32` rounding and small enough to keep every sum exact.
const BASE_DELTA: f64 = 7.0;

/// Family B (`recompute/one_attr` vs `recompute/all_attrs`, BENCH-PLAN §2B) decides on the ratio
/// `cost(all) / (A * cost(one))`, and that ratio means something only if the two shapes differ in one
/// thing: how many attributes are dirty when the timed tick runs. So both halves are checked per
/// attribute — with everything dirty every attribute's value moves, and with one attribute dirty
/// exactly that one moves while the others come back bit-identical (`assert_eq!`, not an epsilon: an
/// attribute the walk never reached is the same `f32` it was, so any difference means it was touched).
///
/// Re-resolving a settled attribute leaves its value where it was, so "moves" needs a write to move:
/// whichever attributes must change get `set_base(+BASE_DELTA)` before the single `run_fixed_tick` the
/// benches loop over.
///
/// The dirty set is asserted per attribute at every step, including *after* the tick. A resolve writes
/// through `&mut T`, `Notify` reads that as a change and re-marks, so the tick that removes `Dirty<T>`
/// puts it back. If that fan-out ever reached a second attribute, a `dirty=one` id would be timing an
/// all-dirty world and its ratio would be 1/A of the truth; the asserts after the tick are what catch
/// it.
#[test]
fn test_recompute_ratio() {
    let spec = recompute::recompute_spec(RECOMPUTE_ATTRIBUTES, RECOMPUTE_EFFECTS);

    // Positive control for the hole in `common.rs`'s `dirty_fraction` doc: a built world is dirty on
    // every attribute whatever `dirty_fraction` says, which is why the shape clears markers instead of
    // ticking them away.
    let mut fresh = build_app_with(&spec, common::ATTRIBUTE_POOL);
    let pool: &'static [AttrSpec] = fresh.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        RECOMPUTE_ATTRIBUTES,
        "shape {} registered {} attributes",
        spec.shape_id(),
        pool.len()
    );
    let fresh_actors = actor_entities(fresh.world_mut());
    assert_eq!(
        fresh_actors.len(),
        spec.actors,
        "shape {} built {} actors",
        spec.shape_id(),
        fresh_actors.len()
    );
    let dirty_at_build = recompute::dirty_names(fresh.world(), pool, fresh_actors[0]);
    assert_eq!(
        dirty_at_build,
        pool_names(pool),
        "the built world is dirty on {} of {} attributes ({dirty_at_build:?}): the claim that a \
         fresh build is dirty on all of them is what makes `clear_dirty` necessary in \
         recompute_shape, so if it is false the settle has to be rethought",
        dirty_at_build.len(),
        RECOMPUTE_ATTRIBUTES
    );

    // (a) every attribute dirty in one tick: every attribute's current value moves.
    let mut all = recompute::recompute_shape(&spec, recompute::Dirtied::All);
    let added = {
        let harness = all.app.world().resource::<HarnessWorld>();
        (0..harness.attrs().len())
            .map(|index| permanent_add(&harness.plan, index))
            .collect::<Vec<f64>>()
    };
    for (index, add) in added.iter().enumerate() {
        assert!(
            *add > 0.0,
            "{} of shape {} draws no permanent Add from the {RECOMPUTE_EFFECTS} effects, so nothing \
             can move its current_value and half (a) cannot be shown for it",
            pool[index].name,
            spec.shape_id()
        );
    }

    let actor = all.actor;
    let before = current_values(all.app.world(), pool, actor);
    assert_eq!(
        recompute::dirty_names(all.app.world(), pool, actor),
        pool_names(pool),
        "dirty=all must dirty every attribute"
    );
    recompute::assert_dirty_attrs(
        all.app.world(),
        pool,
        actor,
        recompute::Dirtied::All,
        "(a) before its tick",
    );
    for attr in pool {
        let base = attr.base(all.app.world(), actor).unwrap();
        attr.set_base(all.app.world_mut(), actor, base + BASE_DELTA);
    }
    run_fixed_tick(all.app.world_mut());
    let after = current_values(all.app.world(), pool, actor);
    for (index, attr) in pool.iter().enumerate() {
        assert_ne!(
            after[index], before[index],
            "{} was dirty and its tick left current_value at {}: the base was written by \
             {BASE_DELTA}, so the resolve did not run and this shape is not timing an all-dirty tick",
            attr.name, before[index]
        );
        assert!(
            (after[index] - before[index] - BASE_DELTA).abs() < 1e-3,
            "{} moved by {} instead of {BASE_DELTA}: {} -> {}",
            attr.name,
            after[index] - before[index],
            before[index],
            after[index]
        );
    }
    recompute::assert_dirty_attrs(
        all.app.world(),
        pool,
        actor,
        recompute::Dirtied::All,
        "(a) after its tick",
    );

    // (b) one attribute dirty: exactly it moves, the rest are identical before and after.
    let mut one = recompute::recompute_shape(&spec, recompute::Dirtied::One);
    let actor = one.actor;
    let before = current_values(one.app.world(), pool, actor);
    for (left, left_value) in before.iter().enumerate() {
        for (right, right_value) in before.iter().enumerate() {
            assert!(
                left == right || left_value != right_value,
                "attributes {left} and {right} of shape {} start on the same current_value \
                 {left_value}, so a move could not be told from a copy: the pool needs \
                 distinguishable base values",
                spec.shape_id()
            );
        }
    }
    // literal, since `pick` is also the oracle of `assert_dirty_attrs`
    assert_eq!(
        recompute::dirty_names(one.app.world(), pool, actor),
        vec![pool[0].name],
        "dirty=one must dirty exactly the pool's first attribute"
    );
    recompute::assert_dirty_attrs(
        one.app.world(),
        pool,
        actor,
        recompute::Dirtied::One,
        "(b) before its tick",
    );

    let moved = &pool[0];
    let base_before = moved.base(one.app.world(), actor).unwrap();
    moved.set_base(one.app.world_mut(), actor, base_before + BASE_DELTA);
    moved.mark_dirty(one.app.world_mut(), actor);
    run_fixed_tick(one.app.world_mut());

    let after = current_values(one.app.world(), pool, actor);
    assert_eq!(
        moved.base(one.app.world(), actor).unwrap(),
        base_before + BASE_DELTA,
        "the base write on {} was lost",
        moved.name
    );
    assert_ne!(
        after[0], before[0],
        "{} was the one dirty attribute and its tick left current_value at {}: the dirty attribute \
         did not resolve, so the shape cannot price a walk",
        moved.name, before[0]
    );
    for index in 1..before.len() {
        assert_eq!(
            after[index], before[index],
            "{} moved from {} to {} on the tick that dirtied only {}: the walk is not \
             attribute-local, so `recompute/one_attr` does not hold the other attributes still and \
             the all/one ratio does not measure what §2B says it measures",
            pool[index].name, before[index], after[index], moved.name
        );
    }

    // The trap the ids have to be honest about: the resolve re-marked `moved` and re-marks it on every
    // later tick, so `dirty=one` means "one attribute *permanently* dirty". It only stays that way if
    // the fan-out stops at the attribute that resolved, which is what repeats here rather than once.
    for tick_number in 1..=3 {
        recompute::assert_dirty_attrs(
            one.app.world(),
            pool,
            actor,
            recompute::Dirtied::One,
            &format!("(b) {tick_number} tick(s) in"),
        );
        run_fixed_tick(one.app.world_mut());
    }
}

/// Every attribute name of a pool, in pool order — the same shape [`recompute::dirty_names`] returns,
/// so the two compare.
fn pool_names(pool: &[AttrSpec]) -> Vec<&'static str> {
    pool.iter().map(|attr| attr.name).collect()
}

/// The actors of `actors` that carry at least one `Dirty<T>`, each with the attribute names that
/// carry it. Per attribute rather than an entity count, because `dirty=25` claims a share of the
/// *attributes* of those actors, not a share of the entities.
fn dirty_by_actor(
    world: &World,
    pool: &[AttrSpec],
    actors: &[Entity],
) -> Vec<(Entity, Vec<&'static str>)> {
    actors
        .iter()
        .map(|actor| (*actor, recompute::dirty_names(world, pool, *actor)))
        .filter(|(_, names)| !names.is_empty())
        .collect()
}

/// `mark_dirty_fraction` is what puts `dirty=N` into a bench id, so two of its claims have to be
/// readable without doing anything to the world first: the count it returns is a count a caller can
/// see, and it lands on exactly that many actors.
///
/// Both were false as committed. `AttrSpec::mark_dirty` is a `World::trigger`, and the observer's
/// `try_insert(Dirty<T>)` (`vitality_effects/src/systems.rs:31-42`) stays in the world's deferred
/// command queue until something flushes it, so the call used to report 2 marks on a world that still
/// showed 0 — and `build_app_with` ends with that call, so every `dirty=N` id rested on marks nobody
/// had made visible. The assert below is the guard: it reads the dirty set back with no flush, no
/// tick, nothing else in between.
///
/// What the shape starts from matters: a built world is dirty on *every* attribute of *every* actor
/// (`test_recompute_ratio` pins that), so dirtiness is only attributable to this call on a settled,
/// proved-clean world — hence `floor::prepare_floor`. The other thing pinned here is that the marks
/// never decay: a resolve writes `&mut T` and `on_change_notify_attribute_parents::<T>`
/// (`vitality_effects/src/systems.rs:274-283`) answers that write with a fresh `MarkNodeDirty`, so
/// the tick that removes a `Dirty<T>` puts it straight back. They also do not spread: exactly the
/// marked actors are still dirty two ticks later, never their neighbours.
#[test]
fn mark_dirty_fraction_marks_are_visible() {
    let spec = WorldSpec {
        attributes_per_actor: 3,
        effects_per_actor: 2,
        modifiers_per_effect: 1,
        actors: 4,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = build_app_with(&spec, common::ATTRIBUTE_POOL);
    let cleared = floor::prepare_floor(&mut app);
    let pool: &'static [AttrSpec] = app.world().resource::<HarnessWorld>().attrs();
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        spec.actors,
        "the shape built {} actors",
        actors.len()
    );
    assert!(
        cleared > 0,
        "the build left no marker to clear, so a 0 below proves nothing (see test_recompute_ratio)"
    );
    assert!(
        dirty_by_actor(app.world(), pool, &actors).is_empty(),
        "a prepared shape is not clean: {:#?}",
        dirty_by_actor(app.world(), pool, &actors)
    );

    let mut rng = common::seed_rng();
    let marked = common::mark_dirty_fraction(app.world_mut(), pool, &actors, 0.5, &mut rng);

    let dirty = dirty_by_actor(app.world(), pool, &actors);
    assert_eq!(
        marked,
        spec.actors / 2,
        "a 0.5 fraction over {} actors marked {marked}",
        spec.actors
    );
    assert_eq!(
        dirty.len(),
        marked,
        "mark_dirty_fraction returned {marked} but the caller can read {} dirty actors back: the \
         marks are still deferred, so a bench id carrying dirty=N is timing whatever the build left, \
         not the N its id counts",
        dirty.len()
    );
    for (actor, names) in &dirty {
        assert_eq!(
            *names,
            pool_names(pool),
            "{actor:?} was picked by a fraction that marks whole actors, but carries {names:?}",
        );
    }

    // The workload a `dirty=N` id claims has to survive the ticks that run it.
    for tick_number in 1..=2 {
        run_fixed_tick(app.world_mut());
        assert_eq!(
            dirty_by_actor(app.world(), pool, &actors),
            dirty,
            "the dirty set changed on tick {tick_number} after mark_dirty_fraction: the shape is \
             drifting out of its own id",
        );
    }
}

/// `recompute::print_ratios` reads criterion's `estimates.json` by hand — `serde_json` is not a
/// dependency of this crate (the workspace declares `serde` only, `serde_json` sits in the lockfile as
/// criterion's own dependency, and `crates/vitality/Cargo.toml` is a protected path), and adding one
/// is not this loop's call. The first version of that parser took the first `"point_estimate"` after
/// the first `"mean"`, which is the *nested* one the moment a CI carries a `point_estimate`, and a
/// truncated file could yield a prefix of some other number. Both fail silently, so the samples below
/// are the ones the parser must get right and the ones it must refuse instead of printing.
#[test]
fn estimate_parser_reads_mean_only() {
    // The file criterion 0.8.2 writes today (`analysis/mod.rs:282-293`): CI first, then the mean's
    // own point estimate, then `median` whose own point estimate is the wrong answer.
    let written = r#"{"mean":{"confidence_interval":{"confidence_level":0.95,"lower_bound":100.0,"upper_bound":200.0},"point_estimate":150.0,"standard_error":10.0},"median":{"confidence_interval":{"confidence_level":0.95,"lower_bound":1.0,"upper_bound":2.0},"point_estimate":9.0,"standard_error":0.5}}"#;
    let mean =
        recompute::parse_estimate(written).expect("the estimates.json criterion writes today");
    assert_eq!(mean.ns, 150.0, "mean.point_estimate");
    assert_eq!(
        mean.ci_half, 50.0,
        "half of the mean's 95 % CI ([100, 200] is ±50)"
    );

    // Same value, adversarial shape: the mean's own point estimate first, and a `point_estimate`
    // nested inside the CI. A scan for the first key after `"mean"` returns 7.0 here.
    let nested = r#"{"mean":{"point_estimate":150.0,"confidence_interval":{"point_estimate":7.0,"lower_bound":100.0,"upper_bound":200.0}}}"#;
    assert_eq!(
        recompute::parse_estimate(nested)
            .expect("a CI that carries its own point_estimate")
            .ns,
        150.0
    );

    // A `mean` that is not a top-level member, a truncated file, an empty file and a quoted number:
    // none of them is a measurement, so none of them may come back as one.
    assert!(
        recompute::parse_estimate(r#"{"extra":{"mean":{"point_estimate":150.0}}}"#).is_none(),
        "a nested `mean` is not criterion's top-level mean"
    );
    assert!(
        recompute::parse_estimate(r#"{"mean":{"confidence_interval":{"lower_bound":100.0,"upp"#)
            .is_none(),
        "a truncated file has no mean"
    );
    assert!(
        recompute::parse_estimate("").is_none(),
        "an empty file has no mean"
    );
    assert!(
        recompute::parse_estimate(
            r#"{"mean":{"confidence_interval":{"lower_bound":100.0,"upper_bound":200.0},"point_estimate":"150"}}"#
        )
        .is_none(),
        "a quoted number is not an estimate"
    );
}

fn est(ns: f64, ci_half: f64) -> recompute::Estimate {
    recompute::Estimate { ns, ci_half }
}

#[test]
fn ratio_row_math_and_noise_guard() {
    use recompute::RatioRow::{Noise, Ratios};
    let none = est(100.0, 5.0);
    // marginal = (all-none)/(a*(one-none)) = 300/(3*50), raw = all/(a*one) = 400/(3*150)
    assert_eq!(
        recompute::ratio_row(3, &none, &est(150.0, 5.0), &est(400.0, 5.0)),
        Ratios {
            raw: 400.0 / 450.0,
            marginal: Some(2.0)
        }
    );
    // gap of exactly the summed half-widths is still noise
    assert_eq!(
        recompute::ratio_row(3, &none, &est(110.0, 5.0), &est(400.0, 5.0)),
        Noise
    );
    // all inside the floor's noise is noise too, the numerator needs a number as well
    assert_eq!(
        recompute::ratio_row(3, &none, &est(150.0, 5.0), &est(108.0, 5.0)),
        Noise
    );
    // a dirty tick cheaper than the clean one outside noise has a raw but no marginal
    assert_eq!(
        recompute::ratio_row(2, &none, &est(50.0, 1.0), &est(300.0, 1.0)),
        Ratios {
            raw: 3.0,
            marginal: None
        }
    );
}

#[test]
fn ratios_in_words_uses_true_median() {
    assert_eq!(
        recompute::ratios_in_words(&[4.0, 1.0, 2.0, 3.0], 4),
        "median 2.50 over 4 rows, min 1.00, max 4.00"
    );
    assert_eq!(
        recompute::ratios_in_words(&[3.0, 1.0, 2.0], 3),
        "median 2.00 over 3 rows, min 1.00, max 3.00"
    );
    assert!(recompute::ratios_in_words(&[1.0], 4).starts_with("no median"));
}

#[test]
fn estimate_refuses_missing_and_stale_files() {
    let root = std::env::temp_dir().join(format!("vitality-est-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(matches!(
        recompute::estimate(&root, "g/x", "a/b"),
        Err(recompute::Refusal::Missing(_))
    ));

    let dir = root.join("g_x").join("a_b").join("new");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("estimates.json");
    std::fs::write(
        &file,
        r#"{"mean":{"confidence_interval":{"lower_bound":100.0,"upper_bound":200.0},"point_estimate":150.0}}"#,
    )
    .unwrap();
    // written just now, so younger than the test binary
    let fresh = recompute::estimate(&root, "g/x", "a/b")
        .ok()
        .expect("fresh file is read");
    assert_eq!((fresh.ns, fresh.ci_half), (150.0, 50.0));

    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert!(matches!(
        recompute::estimate(&root, "g/x", "a/b"),
        Err(recompute::Refusal::Stale(_))
    ));
    let _ = std::fs::remove_dir_all(&root);
}
