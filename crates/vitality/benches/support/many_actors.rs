#![allow(dead_code)]

//! Family B (`recompute/many_actors`, BENCH-PLAN §2B): resolver cost spread over many actors, and
//! the curve that says whether a resolver should shard by actor.
//!
//! The shape is the §2B row and nothing else: `actors` actors, `A` attributes each, **no effects**
//! (`effects_per_actor: 0`), and a `dirty_fraction` of the actors marked dirty on every attribute.
//! The timed unit is one [`common::run_fixed_tick`] — the resolver and nothing else, the same call
//! `floor/idle_tick`, `recompute/one_attr|all_attrs|none_dirty` and `recompute/cross_actor` time —
//! and `Throughput::Elements(actors)` turns it into actors/s.
//!
//! Why the shape is effect-free, which the row does not say out loud: with zero modifiers the only
//! work a dirty attribute has to do is its own `update_attribute` resolve, so the slope against the
//! dirty fraction is the per-actor resolve and the level at `dirty=0` is the schedule floor. Add one
//! effect and every point of the curve carries a walk this row is not trying to price (that is
//! `one_attr`/`all_attrs`, P-6). It also makes the `dirty=0` column comparable with
//! `floor/idle_tick` at the same A and 1000 actors, which is the group's own cross-check.
//!
//! ## The dirty set is set by hand, and it is the thing the guard checks
//!
//! [`common::build_app_with`] returns a world dirty on *every* attribute of *every* actor and
//! unresolved (`common.rs`), so this module settles with [`floor::prepare_floor`] and marks the
//! fraction afterwards with [`common::mark_dirty_fraction`] — which flushes, so the marks are on the
//! entities before anything reads them. The count is then re-read off the world
//! ([`dirty_actors`]) rather than trusted from the marking call, and `tests/test_bench_many_actors.rs`
//! pins it at the grid's own fractions. Marks do not decay (a resolve re-marks what it resolved,
//! `recompute.rs`) and do not spread, so the count the shape starts on is the count every sample
//! runs on; the group's post-loop [`guard::Ran`] guard re-reads it after the timed loop.
//!
//! No cell despawns anything, so the [`common::actor_entities`] list built here stays valid for the
//! whole shape — no despawn means no rebuild, which is what that function's panic rule would
//! otherwise force (see `cross_actor.rs`, which does stagger and rebuild).
//!
//! ## The grid, and the hole in it
//!
//! §2B asks for {100, 1k, 10k} actors × {0, 1, 10, 100} % dirty × A ∈ {5, 20, 40} — 36 ids. That is
//! only affordable if a world build is cheap, and a build is not the cheap part of a 10 k-actor,
//! 40-attribute world: [`floor::prepare_floor`] has to remove `actors × A` `Dirty<T>` components
//! that the spawn observers put there, moving every entity between archetypes each time. So the run
//! decides, in the open:
//!
//! * [`price_setups`] builds and settles one world per (actors, A) pair — all nine, dirty 0 — and
//!   prints the milliseconds. If **every** pair comes in under [`MANY_BUILD_BUDGET_MS`] the grid is
//!   not build-bound and all 36 cells are timed.
//! * otherwise only the shipped cells are timed: the 3 × 4 = 12 (actors × dirty) grid at
//!   `A = MANY_CORE_ATTRS`, plus the A sweep at `actors = MANY_SWEEP_ACTORS` (12 more ids, 4 of them
//!   already counted), i.e. 20 ids. Every skipped cell prints a `NOT TIMED` line carrying its
//!   *measured* setup price and a *projected* mark cost, so the hole is stated in the same units as
//!   the numbers beside it rather than left implicit.
//!
//! Which branch a run takes is printed by the run. The one that landed measured every pair under the
//! budget — 2/6/18 ms at 100 actors, 5/22/54 at 1 k, 37/153/**406** at 10 k, for A = 5/20/40 — so it
//! timed all 36 cells and left no hole. The 10 k/A=40 pair sat 19 % under [`MANY_BUILD_BUDGET_MS`],
//! and `prepare_floor`'s `actors × A` component removals are what scale there, so a slower machine
//! takes the 20-id branch; the table in the group's own doc comment is from the all-36 run.
//!
//! The attribute pool follows A the way `floor/idle_tick` does ([`floor::floor_pool`]): A ≤ 24 comes
//! from `common::ATTRIBUTE_POOL`, A = 40 from `floor::FLOOR_POOL`. The two pools hold different
//! types and the same count, so A = 40 is comparable in A and not in absolute ns with the 5/20
//! columns; `floor/idle_tick` has the same split at its a=40 point.
//!
//! ## Driver label
//!
//! Nothing in a timed number here comes from [`common::tick`]. [`floor::prepare_floor`] does call it
//! once per shape, and that call is setup, so every µs/tick and actors/s below is `run_fixed_tick`
//! only — resolver, no clock, no `Update` systems, no asset work.

use crate::common::{
    AttrSpec, HarnessWorld, WorldSpec, actor_entities, build_app_with, mark_dirty_fraction,
    seed_rng,
};
use crate::floor;
use bevy::prelude::{App, Entity, World};
use std::time::Instant;

/// The actor axis of §2B.
pub const MANY_ACTOR_COUNTS: [usize; 3] = [100, 1_000, 10_000];

/// The dirty axis of §2B, as a share of the actors of the cell.
pub const MANY_FRACTIONS: [f32; 4] = [0.0, 0.01, 0.10, 1.0];

/// The A axis of §2B. 40 is past `common::POOL_LEN` (24), so it draws on `floor::FLOOR_POOL`.
pub const MANY_ATTR_COUNTS: [usize; 3] = [5, 20, 40];

/// The A every shipped (actors × dirty) row is measured at: the middle of the axis, and the A that
/// `recompute/one_attr|all_attrs` and `floor/idle_tick` both have a point at.
pub const MANY_CORE_ATTRS: usize = 20;

/// The actor count the shipped A sweep runs at, for the same reason.
pub const MANY_SWEEP_ACTORS: usize = 1_000;

/// The setup cost above which the 36-cell grid is dropped for the 20 shipped cells. Half a second
/// per world, times 36 worlds and 36 timed loops, is the point where this group stops being the
/// cheapest thing in the run; `MANY_BUILD_BUDGET_MS` is the one number a reader can argue with.
pub const MANY_BUILD_BUDGET_MS: u64 = 500;

/// One cell of the §2B grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub actors: usize,
    pub fraction: f32,
    pub attributes: usize,
}

impl Cell {
    /// The shape this cell times: effect-free, with `dirty_fraction` set so
    /// [`common::WorldSpec::shape_id`]'s `dirty=` field means what it says.
    pub fn spec(self) -> WorldSpec {
        many_spec(self.attributes, self.actors, self.fraction)
    }

    /// The dirty-actor count this cell's fraction asks for. Arithmetic on the fraction, so it is a
    /// claim to check against the world, not a measurement of one.
    pub fn dirty_actors(self) -> usize {
        (self.actors as f32 * self.fraction.clamp(0.0, 1.0)).round() as usize
    }

    /// Whether this cell is timed when the grid is build-bound: the (actors × dirty) grid at
    /// [`MANY_CORE_ATTRS`], or the A sweep at [`MANY_SWEEP_ACTORS`].
    pub fn is_shipped(self) -> bool {
        self.attributes == MANY_CORE_ATTRS || self.actors == MANY_SWEEP_ACTORS
    }

    /// The (actors, A) pair whose setup price this cell's setup is bounded by: `dirty_fraction` adds
    /// marks to an already-built world, so it never makes the build itself slower.
    pub fn pair(self) -> (usize, usize) {
        (self.actors, self.attributes)
    }
}

/// All 36 cells, in the order the group walks them (actors outer, then dirty, then A), so the id
/// order a run prints is the order the axis is written in.
pub fn many_cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for &actors in &MANY_ACTOR_COUNTS {
        for &fraction in &MANY_FRACTIONS {
            for &attributes in &MANY_ATTR_COUNTS {
                cells.push(Cell {
                    actors,
                    fraction,
                    attributes,
                });
            }
        }
    }
    cells
}

/// How many cells [`Cell::is_shipped`] keeps: the number of ids a time-boxed run ships.
pub fn shipped_count() -> usize {
    many_cells().iter().filter(|cell| cell.is_shipped()).count()
}

/// The shape §2B times. `dirty_fraction` is the only dirty thing in it: `build_app_with` applies it
/// on top of a world the spawn observers already marked, and [`many_shape`] re-sets it from zero.
pub fn many_spec(attributes: usize, actors: usize, fraction: f32) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: 0,
        modifiers_per_effect: 0,
        actors,
        dirty_fraction: fraction,
        ticking_fraction: 0.0,
    }
}

/// The id §2B prints. `WorldSpec::shape_id` already carries `actors=` and `dirty=`, both of which
/// are true here (unlike `recompute/one_attr`, where `dirtied_id` has to replace the field), but it
/// cannot carry which of its two dirty counts the world holds — `dirty=1` is one actor at 100 and a
/// hundred at 10 k — nor the driver, so both go in the suffix the way `cross_actor.rs` carries
/// `readers=` and `leaf.rs` carries `excl=`.
pub fn many_id(spec: &WorldSpec, dirty_actors: usize) -> String {
    format!(
        "{}/run_fixed_tick/dirty_actors={dirty_actors}",
        spec.shape_id(),
    )
}

/// A built, settled, marked shape, held whole: the bench times [`common::run_fixed_tick`] on `app`
/// for every sample, so the world has to outlive the closure.
pub struct ManyShape {
    pub app: App,
    pub spec: WorldSpec,
    /// Every actor the shape spawned, sorted by entity index ([`common::actor_entities`]). Read by
    /// the post-loop guard, which is why the shape keeps it.
    pub actors: Vec<Entity>,
    /// Actors [`dirty_actors`] found carrying a `Dirty<T>`, after the settle and the marking. This
    /// is the count the id quotes.
    pub dirty_actors: usize,
    /// `Dirty<T>` components [`floor::prepare_floor`] removed off the build. Zero would mean the
    /// walk that reports a settled world never saw a marker.
    pub markers_cleared: usize,
    /// Build + settle + mark, milliseconds. Setup, not measurement, and printed by the group so the
    /// hole in the grid can be costed in the same run that produced the numbers.
    pub setup_ms: u64,
    /// The [`common::mark_dirty_fraction`] call alone, microseconds — it is milliseconds only on the
    /// 10 k-actor cells — so a skipped cell's mark cost is projected from a measured rate rather
    /// than guessed.
    pub mark_us: u64,
}

/// Builds `spec` from [`floor::floor_pool`], settles it, marks its fraction and proves the count.
pub fn many_shape(spec: &WorldSpec) -> ManyShape {
    let started = Instant::now();
    let pool = floor::floor_pool(spec.attributes_per_actor);
    let mut app = build_app_with(spec, pool);
    let markers_cleared = floor::prepare_floor(&mut app);
    // Collected after the settle: `clear_dirty` removes components, which moves every actor to
    // another archetype. Entity ids survive that, but the list is what the guard reads, so it is
    // taken off the world the guard will look at.
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        spec.actors,
        "shape {} built {} actors",
        spec.shape_id(),
        actors.len(),
    );
    let attrs = app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        attrs.len(),
        spec.attributes_per_actor,
        "shape {} registered {} attributes, not the {} its id names",
        spec.shape_id(),
        attrs.len(),
        spec.attributes_per_actor,
    );
    let marked_at = Instant::now();
    let wanted = mark_dirty_fraction(
        app.world_mut(),
        attrs,
        &actors,
        spec.dirty_fraction,
        &mut seed_rng(),
    );
    let mark_us = marked_at.elapsed().as_micros() as u64;
    let dirty = dirty_actors(app.world(), attrs, &actors);
    assert_eq!(
        dirty.len(),
        wanted,
        "shape {} asked for {wanted} dirty actors and the world carries {}: the id's dirty= and \
         dirty_actors= fields would price a workload the timed tick does not run on",
        spec.shape_id(),
        dirty.len(),
    );
    let setup_ms = started.elapsed().as_millis() as u64;
    ManyShape {
        app,
        spec: spec.clone(),
        actors,
        dirty_actors: dirty.len(),
        markers_cleared,
        setup_ms,
        mark_us,
    }
}

/// The actors of `actors` carrying at least one `Dirty<T>`, in the same sorted order they came in.
///
/// Asked per actor through the pool's `is_dirty` — the same read `recompute::dirty_names` uses —
/// because the count is per *actor* and an attribute-level count would say 40 for one dirty actor at
/// A = 40. `floor::dirty_entity_count` is the independent walk a guard compares it against.
pub fn dirty_actors(world: &World, pool: &[AttrSpec], actors: &[Entity]) -> Vec<Entity> {
    actors
        .iter()
        .copied()
        .filter(|&actor| pool.iter().any(|attr| attr.is_dirty(world, actor)))
        .collect()
}

/// One (actors, A) pair's measured setup cost, with zero marks. Every cell of a pair is bounded by
/// it: marking only adds components to a world that is already built and settled.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    pub actors: usize,
    pub attributes: usize,
    pub ms: u64,
}

impl Setup {
    pub fn line(&self) -> String {
        format!(
            "a={}/actors={}: {} ms",
            self.attributes, self.actors, self.ms
        )
    }
}

/// Prices all nine (actors, A) pairs by building and settling each once at `dirty=0` and dropping
/// it. Runs before any id is timed, because the grid the run ships has to be chosen on measured
/// costs rather than on a guess made when the code was written.
pub fn price_setups() -> Vec<Setup> {
    let mut prices = Vec::new();
    for &actors in &MANY_ACTOR_COUNTS {
        for &attributes in &MANY_ATTR_COUNTS {
            let started = Instant::now();
            let mut app = build_app_with(
                &many_spec(attributes, actors, 0.0),
                floor::floor_pool(attributes),
            );
            floor::prepare_floor(&mut app);
            prices.push(Setup {
                actors,
                attributes,
                ms: started.elapsed().as_millis() as u64,
            });
        }
    }
    prices
}

/// Whether the whole 36-cell grid is affordable: every pair's setup under [`MANY_BUILD_BUDGET_MS`].
pub fn setups_are_cheap(prices: &[Setup]) -> bool {
    !prices.is_empty() && prices.iter().all(|price| price.ms <= MANY_BUILD_BUDGET_MS)
}

/// The priced setup of a cell's pair, or the reason there is none.
pub fn setup_price(prices: &[Setup], cell: Cell) -> Option<&Setup> {
    prices
        .iter()
        .find(|price| (price.actors, price.attributes) == cell.pair())
}

/// The line a skipped cell prints: its setup cost is measured (its pair was priced this run), its
/// mark cost is projected at `mark_rate_ms` — the measured milliseconds per marked
/// (actor, attribute) pair of the shipped cell with the same `actors` and `dirty=`, which exists
/// because every such row ships at `A = MANY_CORE_ATTRS` — and its tick is not measured at all,
/// which is said rather than left out.
pub fn omitted_line(cell: Cell, prices: &[Setup], mark_rate_ms: Option<f64>) -> String {
    let marks = cell.dirty_actors() * cell.attributes;
    let marks_line = match mark_rate_ms {
        Some(rate) => format!(
            "{:.0} ms of marks ({marks} marked attributes at {rate:.3} ms each, projected from the              a={MANY_CORE_ATTRS} row of this actors/dirty pair)",
            marks as f64 * rate,
        ),
        None => format!("{marks} marked attributes, mark cost unprojected (no shipped row)"),
    };
    let setup = match setup_price(prices, cell) {
        Some(price) => format!("{} ms setup (measured, this pair, dirty=0)", price.ms),
        None => format!(
            "no priced setup for a={} and actors={}",
            cell.attributes, cell.actors
        ),
    };
    format!(
        "NOT TIMED {}/dirty_actors={}: {setup} + {marks_line} — the grid was time-boxed to the {} \
         shipped ids, so this cell has no actors/s number in this run",
        cell.spec().shape_id(),
        cell.dirty_actors(),
        shipped_count(),
    )
}
