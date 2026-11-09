//! P-7 (BENCH-PLAN §2B): the `recompute/many_actors` shape carries the dirty fraction its id
//! quotes, on exactly that many actors and on no others.
//!
//! The group's whole deliverable is a curve of actors/s against the dirty fraction, so a fraction
//! bug does not crash anything — it measures a different workload and prints a smooth curve anyway.
//! Two ways that happens and both are pinned here:
//!
//! * the build leaks: `common::build_app_with` hands back a world dirty on *every* attribute of
//!   every actor (`common.rs`), so a shape that only called `mark_dirty_fraction` would time a
//!   100 %-dirty tick under a `dirty=1` label — the `dirty_entity_count == expected` assert below is
//!   what a leaked build trips, and it is an independent archetype walk, not the count the marking
//!   call returned;
//! * the rounding: `dirty=1` at 100 actors is *one* actor and at 10 k actors is a hundred, so the
//!   same id field is a different workload. Both ends of the grid's actor axis are checked, at the
//!   grid's own fractions.
//!
//! The shapes here are the tiny end of the grid (2 attributes, 200 and 100 actors): the claim is
//! about the fraction, and a 10 k-actor build would only make the guard slow.
//! `benches/support/many_actors.rs` says which cells the group actually times.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/floor.rs"]
mod floor;
#[path = "../benches/support/many_actors.rs"]
mod many_actors;

use crate::common::{HarnessWorld, WorldSpec};
use crate::many_actors::{MANY_FRACTIONS, dirty_actors, many_id, many_shape, many_spec};

/// Attributes on the tiny shapes. Two, so an all-or-nothing assert can tell "one attribute of this
/// actor" from "every attribute of it", which is what `mark_dirty_fraction` marks.
const TINY_ATTRS: usize = 2;

/// The tiny actor count: big enough that 1 % is 2 actors rather than a round of 0.01 up to 0, and
/// small enough to build in milliseconds.
const TINY_ACTORS: usize = 200;

#[test]
fn many_actors_dirty_fraction() {
    assert_eq!(
        many_actors::MANY_ACTOR_COUNTS[0],
        100,
        "the grid's smallest actor count moved off 100, so the 1 %-of-100 case below is no longer a \
         grid point",
    );

    let mut probed = Vec::new();
    for fraction in MANY_FRACTIONS {
        let spec = many_spec(TINY_ATTRS, TINY_ACTORS, fraction);
        probed.push(probe_fraction(&spec));
    }

    // The other end of the grid's actor axis: 1 % of the grid's 100-actor column is exactly one
    // actor, which is the cell where a `round` vs `floor` vs `ceil` mistake is easiest to hide.
    let one = many_spec(TINY_ATTRS, 100, 0.01);
    assert_eq!(
        probe_fraction(&one),
        1,
        "the grid's `dirty=1` cell at 100 actors has to be exactly one dirty actor, or the \
         actors/s printed beside it prices a different workload than its id names",
    );

    // The fractions have to be distinguishable at the tiny size, or the loop above proved nothing:
    // 0 and 0.01 would both be "zero dirty actors" on 20 actors, say.
    let counts: Vec<usize> = probed.iter().skip(1).copied().collect();
    let mut sorted = counts.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        counts.len(),
        "the {TINY_ACTORS}-actor shapes came out dirty on {probed:?} actors: two grid fractions \
         built the same workload, so this test cannot tell them apart",
    );
}

/// Builds one tiny shape and asserts its dirty set is *exactly* `round(fraction × actors)` actors —
/// every attribute marked on those, nothing marked on the others — and returns that count.
///
/// Two independent counts are compared: [`dirty_actors`], which asks the pool's `is_dirty` per
/// actor, and `floor::dirty_entity_count`, which walks the archetypes and matches `Dirty<T>` by
/// name. They fail differently — a pool that lost an attribute, and a walk that never sees a marker.
fn probe_fraction(spec: &WorldSpec) -> usize {
    let expected = (spec.actors as f32 * spec.dirty_fraction).round() as usize;
    let shape = many_shape(spec);
    let world = shape.app.world();
    let pool = world.resource::<HarnessWorld>().attrs();

    assert_eq!(
        pool.len(),
        spec.attributes_per_actor,
        "shape {} registered {} attributes, not the {} its a= field names",
        spec.shape_id(),
        pool.len(),
        spec.attributes_per_actor,
    );
    assert_eq!(
        shape.actors.len(),
        spec.actors,
        "shape {} built {} actors",
        spec.shape_id(),
        shape.actors.len(),
    );
    assert!(
        shape.markers_cleared > 0,
        "settling shape {} cleared no Dirty<T>, so the walk that reports it settled never saw a \
         marker and the count below would also read 0 for a world left dirty by the build",
        spec.shape_id(),
    );

    let dirty = dirty_actors(world, pool, &shape.actors);
    assert_eq!(
        dirty.len(),
        expected,
        "shape {} asked for {} % dirty of {} actors = {expected} and got {}: the id would price a \
         workload the world does not carry",
        spec.shape_id(),
        (spec.dirty_fraction * 100.0).round(),
        spec.actors,
        dirty.len(),
    );
    assert_eq!(
        floor::dirty_entity_count(world),
        expected,
        "shape {}: the pool walk counts {} dirty actors and the archetype walk counts {} dirty \
         entities; with zero effects those are the same entities, so one of the two counts is wrong",
        spec.shape_id(),
        dirty.len(),
        floor::dirty_entity_count(world),
    );
    if spec.dirty_fraction < 1.0 {
        assert!(
            dirty.len() < shape.actors.len(),
            "shape {} is dirty on every one of its {} actors while its id says {} %: the build's own \
             markers were never cleared, so the timed tick resolves the whole world",
            spec.shape_id(),
            shape.actors.len(),
            (spec.dirty_fraction * 100.0).round(),
        );
    }

    // All-or-nothing: `mark_dirty_fraction` marks every attribute of the actors it picks, so a
    // partial actor means the group's `dirty_actors=` field is a count of something else.
    for &actor in &dirty {
        let marked = pool
            .iter()
            .filter(|attr| attr.is_dirty(world, actor))
            .count();
        assert_eq!(
            marked,
            pool.len(),
            "shape {}: actor {actor:?} carries {marked} of {} dirty attributes, so the fraction is \
             not a per-actor count and the id's dirty_actors= field would be a fiction",
            spec.shape_id(),
            pool.len(),
        );
    }
    let id = many_id(spec, dirty.len());
    assert!(
        id.contains(&format!("dirty_actors={}", dirty.len())),
        "id {id} does not carry the dirty-actor count the world was just proved to hold",
    );
    dirty.len()
}
