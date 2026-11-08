#![allow(dead_code)]

//! Per-actor identity (`AUTO-REPORT-bench-2026-11-07.md` §4, last bullet).
//!
//! [`common::prepare_world`](crate::common) builds one `ActorBuilder` template and
//! `SpawnActorCommand` (`vitality/src/actors.rs:64-68`) replays its `builder_actions` for every
//! spawn, so every actor a [`WorldSpec`](crate::common::WorldSpec) spawns carries the same value for
//! every attribute. A guard that compares per-actor values — which attribute `mark_dirty_fraction`
//! picked, which attribute a `dirty=one` row left alone, what `many_actors_dirty_fraction` expects at
//! the shard boundary — passes just as loudly when it reads the wrong actor.
//!
//! [`stagger_actors`] is the opt-in step a shape takes to close that: it writes a distinct base per
//! actor per attribute, and `build_app_with`'s generated values are left alone so no landed number
//! moves. It writes through [`AttrSpec::set_base`], a `DerefMut` write, so the write re-marks and
//! re-resolves like any other write and the caller settles the world with one tick of its own.
//!
//! Criterion-free so `tests/test_bench_identity.rs` can include it (rule 3 of
//! `tests/test_bench_wiring.rs`).

use crate::common::{AttrSpec, current_values};
use bevy::prelude::{Entity, World};

/// Lowest base a stagger writes: the floor of the `10.0..100.0` band `prepare_world` draws from, so a
/// staggered shape still reads in the range the landed numbers came from.
const IDENTITY_FLOOR: f32 = 10.0;

/// `f32`-exact step between two neighbouring staggered bases. Exactness is the point: every written
/// base is a multiple of `1/64` below `2^17`, so it survives the resolver's `f32` sums untouched and
/// two actors cannot round onto the same value.
const IDENTITY_STEP: f32 = 1.0 / 64.0;

/// The base for attribute `attr` of actor `actor`: `IDENTITY_FLOOR + (actor * attrs + attr) *
/// IDENTITY_STEP`, so no two (actor, attribute) pairs share a base however deep the shape is.
fn staggered_base(actor: usize, attr: usize, attrs: usize) -> f32 {
    IDENTITY_FLOOR + (actor * attrs + attr) as f32 * IDENTITY_STEP
}

/// Writes one distinct base per actor per attribute and returns how many bases it wrote.
///
/// Only actors already in the world: the stagger is a write, never a spawn, because the shape a bench
/// times has to stay the shape its id names. `attrs` in pool order and `actors` in
/// [`common::actor_entities`](crate::common::actor_entities) order, so the values a guard predicts
/// are the values the stagger wrote.
///
/// A missing attribute is an error, not a skipped write: `AttrSpec::set_base` ignores an entity that
/// does not carry `T`, and a count of writes the world never took is the same fiction
/// `mark_dirty_fraction` documented before it flushed.
pub fn stagger_actors(world: &mut World, attrs: &[AttrSpec], actors: &[Entity]) -> usize {
    assert!(
        !actors.is_empty() && !attrs.is_empty(),
        "identity needs something to distinguish: {} actors and {} attributes",
        actors.len(),
        attrs.len()
    );

    for (index, &actor) in actors.iter().enumerate() {
        for (attr, spec) in attrs.iter().enumerate() {
            assert!(
                spec.base(world, actor).is_some(),
                "{} missing on {actor:?}, so staggering it would write nothing and the guard that \
                 reads it would be reading a value it never moved",
                spec.name
            );
            let base = staggered_base(index, attr, attrs.len());
            spec.set_base(world, actor, base as f64);
        }
    }
    actors.len() * attrs.len()
}

/// Fails unless every pair of actors differs on every attribute, read the way a guard reads it —
/// through [`current_values`], so a stagger whose resolve never ran still leaves the actors equal.
pub fn assert_distinguishable(world: &World, attrs: &[AttrSpec], actors: &[Entity]) {
    assert!(
        actors.len() >= 2 && !attrs.is_empty(),
        "distinguishability needs two actors and one attribute to compare: {} actors and {} \
         attributes",
        actors.len(),
        attrs.len()
    );

    let values: Vec<Vec<f64>> = actors
        .iter()
        .map(|&actor| current_values(world, attrs, actor))
        .collect();

    for (left, left_values) in values.iter().enumerate() {
        for (right, right_values) in values.iter().enumerate().skip(left + 1) {
            for (attr, (a, b)) in left_values.iter().zip(right_values.iter()).enumerate() {
                assert_ne!(
                    a, b,
                    "actors {} and {} are identical on {} ({a}): a guard that reads either one sees \
                     the same number, so stagger_actors has not distinguished them",
                    left, right, attrs[attr].name,
                );
            }
        }
    }
}
