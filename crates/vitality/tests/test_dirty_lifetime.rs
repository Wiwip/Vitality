//! B-4: how long a `Dirty<T>` lives. Both halves were written as characterization of the old resolver
//! and both are now read the same way: *never spreads* still holds, and *never decays* was the bug
//! D-34 removed (`.auto/decisions.md`, one line at `crates/vitality_core/src/math.rs`), so the
//! `still dirty` asserts here were inverted in place rather than deleted — see
//! `AUTO-REPORT-bench-2026-11-07.md` §2 for the probe that settled the old behaviour.
//!
//! The mechanism the old version had, in the order it fired:
//!
//! * `AbsDiff::are_different` for floats was `self.abs_diff(rhs) > Self::Output::MIN` and `MIN` for
//!   `f32` is `f32::MIN` — negative — so the comparison was true for every pair of values, equal ones
//!   included (only a NaN operand made it false);
//! * so `update_attribute` (`crates/vitality_core/src/attributes.rs:326`) called `set_current_value`
//!   even when the recomputed value equalled the current one;
//! * that write made `on_change_notify_attribute_parents::<T>` see `Changed<T>`
//!   (`crates/vitality_effects/src/systems.rs:274-283`) and trigger a fresh `MarkNodeDirty<T>`, whose
//!   observer (`vitality_effects/src/systems.rs:31-42`) put `Dirty<T>` back on the actor — every tick,
//!   forever.
//!
//! With `!=` the loop is broken at its first step: a resolve that yields the value the attribute
//! already holds writes nothing, so no `Changed<T>`, no re-mark, and a marked actor is idle again one
//! tick after it resolves (measured: clean at tick 1, and still clean at tick 22 here). What that does
//! *not* change is the half this file is really guarding — a mark still only rides the `EffectTarget`
//! chain from the entity that changed, so an unmarked actor in this world has nothing to receive, and a
//! decay that leaked sideways would go red on the `never spreads` assert.
//!
//! "Nothing to receive" is about *this* shape, though: its four actors carry effects on themselves
//! only. Probed on `cross_actor`'s shape (one source, one reader, one `EffectSubject::Source`
//! modifier), a mark on the source alone still puts `Dirty<Attr4>` on the reader on the next tick — the
//! cascade doing its job — and since D-34 that mark decays too once the reader's value stops moving
//! (`test_dirty_spread.rs`).
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

/// Both halves, re-read at one point in time: every unmarked actor is still clean, and the marked
/// actors' own marks have drained. The spread check comes first so a leak sideways names itself
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
    assert!(
        dirty_actors.is_empty(),
        "never decays came back: {when} the dirty actors are {dirty_actors:?}, and the marked ones \
         ({marked:?}) should be idle by now. Since D-34 a bare mark resolves once and, because the \
         value it yields is the one the attribute already held, writes nothing and re-marks nothing \
         (`are_different` is `!=`, not the old always-true `abs_diff > f32::MIN`). Read together with \
         the spread loop above: an actor that stays dirty forever is the storm, an actor that is dirty \
         and was never marked is the leak",
    );
}

#[test]
fn marks_decay_and_never_spread() {
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
        "the build left no `Dirty<T>` for `prepare_floor` to see ({} dirty at handoff), so the clean reads \
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
    assert!(
        dirty_by_actor(app.world(), pool, &actors).is_empty(),
        "the marks themselves survived the first two ticks: {baseline:?} was the list at the mark, and \
        the whole thing should be gone by tick 1",
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
    assert!(
        dirty_by_actor(app.world(), pool, &actors).is_empty(),
        "the marks survived {} ticks: {baseline:?} was the list at the mark and nothing should re-raise \
         it once no value moves",
        2 + EXTRA_TICKS,
    );
}
