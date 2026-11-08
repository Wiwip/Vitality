//! B-4: how long a `Dirty<T>` lives — a **characterization** test. Every assert here pins what the
//! resolver does *today*, not what it should do, and the two halves are judged differently once the
//! re-mark is fixed: *never decays* is the bug and is expected to **flip** (the `still dirty` asserts
//! go red, and should then be inverted rather than deleted), while *never spreads* is expected to keep
//! holding. The library fix itself is out of scope (`.auto/decisions.md` D-24), and this file lives on
//! its own because `test_bench_harness.rs` was foreign when B-4 was written (D-23). See
//! `AUTO-REPORT-bench-2026-11-07.md` §2 for the probe that settled it.
//!
//! The mechanism, in the order it fires:
//!
//! * `AbsDiff::are_different` for floats is `self.abs_diff(rhs) > Self::Output::MIN`
//!   (`crates/vitality_core/src/math.rs:32-33`), and `MIN` for `f32` is `f32::MIN` — negative — so the
//!   comparison is true for every pair of values except a NaN operand;
//! * so `update_attribute` (`crates/vitality_core/src/attributes.rs:326`) calls `set_current_value`
//!   even when the recomputed value equals the current one;
//! * that write makes `on_change_notify_attribute_parents::<T>` see `Changed<T>`
//!   (`crates/vitality_effects/src/systems.rs:274-283`) and trigger a fresh `MarkNodeDirty<T>`, whose
//!   observer (`vitality_effects/src/systems.rs:31-42`) puts `Dirty<T>` back on the actor.
//!
//! Net effect: an actor once marked is never idle again. What makes that bearable is that the mark
//! stays on the actor that got it — `mark_node_dirty_observer` walks up `EffectTarget` from the entity
//! that changed, so an unmarked actor in the same world has nothing to receive, and that is the half
//! this file is really guarding: a fix that makes the marks decay must not make them leak sideways.
//! "Nothing to receive" is about *this* shape, though: its four actors carry effects on themselves
//! only. Probed on `cross_actor`'s shape (one source, one reader, one `EffectSubject::Source`
//! modifier), a mark on the source alone puts `Dirty<Attr4>` on the reader on the next tick — which is
//! the cascade doing its job — and that mark then never decays either, for the same reason as the
//! source's.
//!
//! The shape is settled with [`floor::prepare_floor`] — one tick, [`floor::clear_dirty`], one idle
//! tick and an assert that nothing survived — because `build_app_with` hands back a world the spawn
//! observers already marked on every attribute, so marks from `mark_dirty_fraction` are only
//! attributable to those calls on a world proved clean first.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/floor.rs"]
mod floor;

use crate::common::{
    AttrSpec, HarnessWorld, WorldSpec, actor_entities, build_app_with, mark_dirty_fraction,
    seed_rng, tick,
};
use crate::floor::{dirty_entity_count, prepare_floor};
use bevy::prelude::{Entity, World};

/// Small on purpose: the only variable this file moves is who is dirty, so everything else stays at
/// the harness's tiny shape (3 attributes, 2 permanent effects x 1 modifier, so a tick has a real
/// walk to run and `ticking_fraction` 0.0 keeps `Time<Fixed>` out of it).
const ACTORS: usize = 4;
const ATTRIBUTES: usize = 3;

/// The owner's "mark 2 of 4": `mark_dirty_fraction` rounds, so this fraction marks exactly two.
const MARKED: usize = 2;

/// Extra horizon beside the two ticks the item asks for: the title says "never", so one tick proves
/// the mark survived and twenty more show nothing slowly drains it.
const EXTRA_TICKS: usize = 20;

fn b4_spec() -> WorldSpec {
    WorldSpec {
        attributes_per_actor: ATTRIBUTES,
        effects_per_actor: 2,
        modifiers_per_effect: 1,
        actors: ACTORS,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// The actors carrying at least one `Dirty<T>`, each with the attribute names carrying it, in actor
/// order. Read per attribute rather than as an entity count, so an actor that lost one mark of three
/// shows up with a shorter name list instead of looking unchanged.
fn dirty_by_actor(
    world: &World,
    pool: &[AttrSpec],
    actors: &[Entity],
) -> Vec<(Entity, Vec<&'static str>)> {
    actors
        .iter()
        .copied()
        .map(|actor| {
            let names: Vec<&'static str> = pool
                .iter()
                .filter(|attr| attr.is_dirty(world, actor))
                .map(|attr| attr.name)
                .collect();
            (actor, names)
        })
        .filter(|(_, names)| !names.is_empty())
        .collect()
}

fn pool_names(pool: &[AttrSpec]) -> Vec<&'static str> {
    pool.iter().map(|attr| attr.name).collect()
}

/// The per-actor mark counts the report quotes. Printed, not asserted: the asserts below already say
/// who is dirty, and the world walk beside them is the world-level read — it reports the two marked
/// actors and nothing else, so no effect node is quietly dirty too.
fn report(label: &str, world: &World, pool: &[AttrSpec], actors: &[Entity]) {
    let per_actor: Vec<String> = actors
        .iter()
        .map(|actor| {
            let marks = pool
                .iter()
                .filter(|attr| attr.is_dirty(world, *actor))
                .count();
            format!("{actor:?}={marks}")
        })
        .collect();
    println!(
        "B-4 {label}: {} of {ACTORS} actors dirty [{per_actor:?}]; world walk sees {} dirty entities",
        dirty_by_actor(world, pool, actors).len(),
        dirty_entity_count(world),
    );
}

/// Both halves, re-read at one point in time: every unmarked actor is still clean, and the dirty set
/// is still exactly the marked one. The spread check comes first so a leak sideways names itself
/// rather than showing up as a mismatched dirty list.
fn assert_both_halves(
    world: &World,
    pool: &[AttrSpec],
    actors: &[Entity],
    marked: &[Entity],
    when: &str,
) {
    let clean: Vec<Entity> = actors
        .iter()
        .copied()
        .filter(|actor| !marked.contains(actor))
        .collect();
    assert_eq!(
        clean.len(),
        ACTORS - MARKED,
        "the shape has {} actors and {} of them are marked, so it cannot name {} clean ones",
        actors.len(),
        marked.len(),
        ACTORS - MARKED,
    );
    for actor in &clean {
        let marks: Vec<&'static str> = pool
            .iter()
            .filter(|attr| attr.is_dirty(world, *actor))
            .map(|attr| attr.name)
            .collect();
        assert!(
            marks.is_empty(),
            "never spreads: the actor {actor:?} nobody marked carries {marks:?} {when}",
        );
    }

    let dirty = dirty_by_actor(world, pool, actors);
    let dirty_actors: Vec<Entity> = dirty.iter().map(|(actor, _)| *actor).collect();
    assert_eq!(
        dirty_actors, marked,
        "never decays: {when} the dirty actors are {dirty_actors:?} and not the marked \
         {marked:?} — if a resolver fix made the marks decay, invert this assert rather than delete \
         it (the module docs say that is the expected flip)",
    );
}

#[test]
fn marks_never_decay_and_never_spread() {
    let spec = b4_spec();
    let mut app = build_app_with(&spec, common::ATTRIBUTE_POOL);
    let cleared = prepare_floor(&mut app);
    let pool: &'static [AttrSpec] = app.world().resource::<HarnessWorld>().attrs();
    let actors = actor_entities(app.world_mut());

    assert_eq!(
        actors.len(),
        ACTORS,
        "shape {} built {} actors",
        spec.shape_id(),
        actors.len()
    );
    assert_eq!(
        pool.len(),
        ATTRIBUTES,
        "the shape registered {} attributes, not the {} its id says",
        pool.len(),
        ATTRIBUTES,
    );
    assert!(
        cleared > 0,
        "the build left no `Dirty<T>` for `clear_dirty` to remove ({} cleared), so the clean reads \
         below are a walk that never saw a marker",
        cleared,
    );
    assert_eq!(
        dirty_entity_count(app.world()),
        0,
        "a settled shape is not clean, so nothing below can attribute a mark to this file's own call"
    );

    let mut rng = seed_rng();
    let marked = mark_dirty_fraction(
        app.world_mut(),
        pool,
        &actors,
        MARKED as f32 / ACTORS as f32,
        &mut rng,
    );
    assert_eq!(
        marked, MARKED,
        "a {}/{} fraction over {ACTORS} actors marked {marked}",
        MARKED, ACTORS,
    );

    let baseline = dirty_by_actor(app.world(), pool, &actors);
    let marked_actors: Vec<Entity> = baseline.iter().map(|(actor, _)| *actor).collect();
    assert_eq!(
        baseline.len(),
        MARKED,
        "`mark_dirty_fraction` counted {marked} actors but {} are readable: either its `flush()` is \
         gone and a bench id carrying dirty=N counts marks nobody can see, or more actors got marked \
         than the fraction picked",
        baseline.len(),
    );
    for (actor, names) in &baseline {
        assert_eq!(
            *names,
            pool_names(pool),
            "{actor:?} was picked by a helper that marks whole actors but carries {names:?}",
        );
    }
    report("after mark (flushed)", app.world(), pool, &actors);

    // The owner's two ticks.
    for _ in 1..=2 {
        tick(&mut app);
    }
    report("at tick 2", app.world(), pool, &actors);
    assert_both_halves(
        app.world(),
        pool,
        &actors,
        &marked_actors,
        "two ticks after the mark",
    );
    assert_eq!(
        dirty_by_actor(app.world(), pool, &actors),
        baseline,
        "the marks themselves moved within the first two ticks, not just their actors",
    );

    for _ in 1..=EXTRA_TICKS {
        tick(&mut app);
    }
    report(
        &format!("at tick {}", 2 + EXTRA_TICKS),
        app.world(),
        pool,
        &actors,
    );
    assert_both_halves(
        app.world(),
        pool,
        &actors,
        &marked_actors,
        &format!("{} ticks after the mark", 2 + EXTRA_TICKS),
    );
    assert_eq!(
        dirty_by_actor(app.world(), pool, &actors),
        baseline,
        "the marks themselves moved over {} ticks, not just their actors",
        2 + EXTRA_TICKS,
    );
}
