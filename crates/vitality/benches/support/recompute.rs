#![allow(dead_code)]

//! Family B (`BENCH-PLAN.md` §2B): is the effect walk repeated once per attribute (T2)?
//!
//! One actor, A attributes, E permanent effects x 1 modifier each, and the only thing the two groups
//! change is how many of the A attributes carry a `Dirty<T>` when the timed tick runs: one
//! ([`Dirtied::One`]) or all ([`Dirtied::All`]). The deliverable is the ratio
//! `cost(all) / (A * cost(one))`, which [`print_ratios`] prints per A from the `estimates.json` files
//! criterion wrote for those very ids.
//!
//! The `dirty=` field of the id is the whole experiment, so [`recompute_shape`] proves it per
//! attribute instead of asserting an entity count. Two facts about the dirty set are load-bearing:
//!
//! * `build_app_with` hands back a world dirty on *every* attribute and unresolved (its
//!   `dirty_fraction` is applied *on top* of the markers the `Insert<T>` observers already put
//!   there), so the shape settles with a `tick` and then [`floor::clear_dirty`]. A shape that only
//!   ticked would be timing all-A resolve work while its id says `dirty=one`.
//! * an attribute that resolves re-dirties itself: `update_attribute` writes through `&mut T`
//!   (`vitality_core/src/attributes.rs:315`) and `on_change_notify_attribute_parents::<T>` in
//!   `EffectsSet::Notify` (`vitality_effects/src/systems.rs:274`) reads that as a change and triggers
//!   a fresh `MarkNodeDirty`. So `one_attr` is "one attribute *permanently* dirty" and `all_attrs` is
//!   the realistic steady state — the ids say `dirty=one` / `dirty=all` with that meaning, and
//!   [`assert_dirty_attrs`] after the timed loop is what keeps `dirty=one` from silently becoming
//!   `dirty=all`.

use crate::common::{
    ATTRIBUTE_POOL, AttrSpec, HarnessWorld, WorldSpec, actor_entities, build_app_with,
};
use crate::floor;
use bevy::prelude::{App, Entity, World};
use std::path::{Path, PathBuf};

/// The A axis of §2B. 20 is the deepest attribute count the default pool serves without declaring a
/// second pool, and `common.rs` says so.
pub const RECOMPUTE_ATTR_COUNTS: [usize; 3] = [1, 5, 20];

/// The E axis of §2B: permanent effects, each carrying exactly one modifier.
pub const RECOMPUTE_EFFECT_COUNTS: [usize; 4] = [1, 5, 20, 100];

/// How many of the shape's attributes the timed tick finds dirty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dirtied {
    /// The floor: the same world, nothing dirty. §2B's own formula subtracts `idle(A)`; this is that
    /// term, measured inside the shape instead of borrowed from `floor/idle_tick`'s 1000 actors.
    None,
    One,
    All,
}

impl Dirtied {
    /// The `dirty=` field of the id.
    pub fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::One => "one",
            Self::All => "all",
        }
    }

    /// The group this shape is timed in.
    pub fn group(self) -> &'static str {
        match self {
            Self::None => "recompute/none_dirty",
            Self::One => "recompute/one_attr",
            Self::All => "recompute/all_attrs",
        }
    }

    /// The attributes of `pool` this shape leaves dirty. `One` is always the pool's first entry, so
    /// the ratio's two sides dirty the same attribute of the same generated world.
    pub fn pick<'a>(self, pool: &'a [AttrSpec]) -> &'a [AttrSpec] {
        match self {
            Self::None => &pool[..0],
            Self::One => &pool[..1],
            Self::All => pool,
        }
    }
}

/// The shape §2B times: `actors` is 1 because the ratio is about one actor's A attribute storages,
/// and `dirty_fraction` is 0 because the dirty set is set per attribute by [`recompute_shape`], never
/// by the generator's seeded fraction.
pub fn recompute_spec(attributes: usize, effects: usize) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: effects,
        modifiers_per_effect: 1,
        actors: 1,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// The id §2B wants: `common::shape_id` with its `dirty=` field replaced by the set the shape was
/// proved to carry. Replacing rather than reformatting, so the axis order and every other field stay
/// `common.rs`'s.
pub fn dirtied_id(spec: &WorldSpec, dirtied: Dirtied) -> String {
    let plain = spec.shape_id();
    let id = plain.replace("/dirty=0", &format!("/dirty={}", dirtied.tag()));
    assert!(
        id != plain,
        "shape {plain} carries no /dirty=0 field, so the dirty set cannot be put in its id: the \
         generator would be leaving actors dirty that the id does not count"
    );
    id
}

/// A settled shape with its dirty set marked and proved. Held whole: the benches time
/// `run_fixed_tick` on `app` for hundreds of samples, so the world has to outlive the closure.
pub struct RecomputeShape {
    pub app: App,
    pub actor: Entity,
    pub dirtied: Dirtied,
    /// `Dirty<T>` components [`floor::clear_dirty`] removed inside [`floor::prepare_floor`]. Zero
    /// would mean the shape never had markers to clear, i.e. the build did not dirty anything and the
    /// positive control in `test_recompute_ratio` is wrong.
    pub markers_cleared: usize,
}

/// Builds `spec` from the default pool, settles it with [`floor::prepare_floor`] (tick, clear, one
/// idle `run_fixed_tick` that must leave zero dirty), marks the attributes [`Dirtied::pick`] names,
/// and asserts the per-attribute dirty state it claims.
pub fn recompute_shape(spec: &WorldSpec, dirtied: Dirtied) -> RecomputeShape {
    let mut app = build_app_with(spec, ATTRIBUTE_POOL);
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        spec.actors,
        "shape {} built {} actors: the ratio holds the actor count at 1 so it prices one actor's A \
         attribute storages",
        spec.shape_id(),
        actors.len(),
    );
    let actor = actors[0];
    let markers_cleared = floor::prepare_floor(&mut app);
    let pool = app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        spec.attributes_per_actor,
        "shape {} registered {} attributes",
        spec.shape_id(),
        pool.len(),
    );
    for attr in dirtied.pick(pool) {
        attr.mark_dirty(app.world_mut(), actor);
        // `AttrSpec::mark_dirty` is a `World::trigger`, and the observer's `try_insert(Dirty<T>)` sits
        // in the world's deferred queue until something flushes it: without this the mark is invisible.
        app.world_mut().flush();
    }
    assert_dirty_attrs(app.world(), pool, actor, dirtied, "after marking");
    RecomputeShape {
        app,
        actor,
        dirtied,
        markers_cleared,
    }
}

/// The attributes of `pool` carrying a `Dirty<T>` on `actor`, by name. Per attribute, because an
/// entity count cannot tell "the one I marked" from "everything the fan-out re-marked".
pub fn dirty_names(world: &World, pool: &[AttrSpec], actor: Entity) -> Vec<&'static str> {
    pool.iter()
        .filter(|attr| attr.is_dirty(world, actor))
        .map(|attr| attr.name)
        .collect()
}

/// The dirty set on `actor` is exactly [`Dirtied::pick`]'s. Every call site names the moment it
/// checks, because the same call runs before marking, after marking and after the timed loop.
pub fn assert_dirty_attrs(
    world: &World,
    pool: &[AttrSpec],
    actor: Entity,
    dirtied: Dirtied,
    when: &str,
) {
    let expected: Vec<&'static str> = dirtied.pick(pool).iter().map(|a| a.name).collect();
    let got = dirty_names(world, pool, actor);
    assert_eq!(
        got, expected,
        "{when}: id `dirty={}` holds {} dirty attribute(s) {got:?}, expected the {} of {:?}",
        dirtied.tag(),
        got.len(),
        dirtied.tag(),
        expected,
    );
}

/// The file criterion 0.8 writes for one benchmark: `<root>/<group>/<id>/new/estimates.json`, where
/// every `/` of the id becomes `_` (`criterion-0.8.2/src/report.rs:172`,
/// `BenchmarkId::directory_name`). Kept separate so a missing estimate can print the path it wanted.
fn estimates_path(root: &Path, group: &str, bench_id: &str) -> PathBuf {
    root.join(group.replace('/', "_"))
        .join(bench_id.replace('/', "_"))
        .join("new")
        .join("estimates.json")
}

/// Where criterion writes its output. Criterion resolves its own target directory rather than using
/// one relative to the bench's cwd, so the only reliable way to find it is to look: `CARGO_TARGET_DIR`
/// first, then `target/criterion` in the cwd and in every ancestor of it. `None` means nothing this
/// process can see was written there, which `print_ratios` says out loud rather than printing a ratio
/// it did not measure.
pub fn criterion_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(target).join("criterion"));
    }
    let Ok(cwd) = std::env::current_dir() else {
        return None;
    };
    candidates.push(cwd.join("target/criterion"));
    let mut ancestor = cwd;
    while ancestor.pop() {
        candidates.push(ancestor.join("target/criterion"));
    }
    candidates.into_iter().find(|path| path.is_dir())
}

/// `mean.point_estimate` (ns) from the estimates file criterion wrote for `group`/`bench_id`, or
/// `None`. Criterion 0.8 stores each benchmark as it finishes (`analysis/mod.rs:171`, gated on
/// `should_save_baseline`, true unless `--baseline`/`--save-baseline`/`--load-baseline` is passed), so
/// the `one_attr` files are on disk before the `all_attrs` side of the same A is read. Parsed by hand:
/// `serde_json` is not a dependency of this crate and none may be added.
pub fn estimate_ns(root: &Path, group: &str, bench_id: &str) -> Option<f64> {
    let text = std::fs::read_to_string(estimates_path(root, group, bench_id)).ok()?;
    let mean = text.find("\"mean\"")?;
    let key = "\"point_estimate\":";
    let start = mean + text[mean..].find(key)? + key.len();
    let tail = &text[start..];
    let end = tail.find([',', '}']).unwrap_or_else(|| tail.len().min(40));
    tail[..end].trim().parse().ok()
}

/// One measured point of one side of the ratio: the shape and the criterion id it was timed under.
pub struct Point {
    pub spec: WorldSpec,
    pub bench_id: String,
}

/// Prints, per E, the raw `cost(all) / (A * cost(one))` and the floor-subtracted
/// `(all - none) / (A * (one - none))`, reading the estimates criterion wrote for the ids it just
/// timed. This *is* the §2B deliverable: the raw `time:` lines in criterion's own summary carry the
/// shape, so nothing here restates them, and a missing estimate prints the path it wanted so a
/// `--baseline` run cannot pass for a measured one.
///
/// §2B's own row subtracts `idle(A)` — "so T1's floor is not counted as walk cost" — so
/// [`marginal`] is the ratio §2B means, and the raw one is kept only because it is the number P-6's
/// item text asks for. It is not a small correction: at A=20 the idle schedule is most of `one`, so
/// the raw ratio is arithmetic on dead work.
///
/// A shape whose dirty tick costs no more than its clean tick gets `marginal=0`: a denominator at or
/// below zero cannot invent a ratio.
///
/// Called after every side of the A has been timed, because the files only exist once criterion has
/// finished the benchmark that writes them.
pub fn print_ratios(a: usize, none: &[Point], one: &[Point], all: &[Point]) {
    let root = criterion_root();
    println!(
        "recompute ratio table (A={a}, ns per run_fixed_tick, estimates under {root:?}): \
         marginal=(all-none)/(A*(one-none)) is BENCH-PLAN §2B's ratio, none_dirty being the \
         same world with zero attributes dirty (its idle schedule, §2A); raw=all/(A*one) is the \
         item's simpler form, which counts that idle schedule as walk cost:"
    );
    let mut raws = Vec::new();
    let mut marginals = Vec::new();
    for ((none, one), all) in none.iter().zip(one.iter()).zip(all.iter()) {
        assert_eq!(
            one.spec.shape_id(),
            all.spec.shape_id(),
            "the sides of the ratio were measured on different shapes"
        );
        // The full criterion ids of the pair, twice over on purpose: no number may travel without the
        // shape it came from (BENCH-PLAN §1.5), and the row's `dirty=` must match the ids it reads.
        let id = format!(
            "{} vs {}",
            full_id(Dirtied::One, &one.bench_id),
            full_id(Dirtied::All, &all.bench_id)
        );
        let ns = |dirtied: Dirtied, point: &Point| {
            root.as_ref()
                .and_then(|root| estimate_ns(root, dirtied.group(), &point.bench_id))
        };
        let (Some(none_ns), Some(one_ns), Some(all_ns)) =
            (ns(Dirtied::None, none), ns(Dirtied::One, one), ns(Dirtied::All, all))
        else {
            println!(
                "  {id}: no estimate at {:?} (a --baseline or --load-baseline run writes neither)",
                root.as_ref()
                    .map(|root| estimates_path(root, Dirtied::All.group(), &all.bench_id))
            );
            continue;
        };
        let raw = all_ns / (a as f64 * one_ns);
        raws.push(raw);
        let walk = one_ns - none_ns;
        let marginal = if walk > 0.0 {
            marginals.push((all_ns - none_ns) / (a as f64 * walk));
            format!("{:.2}", (all_ns - none_ns) / (a as f64 * walk))
        } else {
            "0 (one<=none: a dirty tick cost no more than the clean one)".to_string()
        };
        println!(
            "  {id}: none={none_ns:.0} one={one_ns:.0} all={all_ns:.0} raw={raw:.2} marginal={marginal}"
        );
    }
    println!(
        "  A={a}: raw {}; marginal {}",
        ratios_in_words(&raws),
        ratios_in_words(&marginals)
    );
}

/// `median 0.07 over 4 points, min 0.06, max 0.12` — or the reason there is no number.
fn ratios_in_words(ratios: &[f64]) -> String {
    if ratios.is_empty() {
        return "no ratio (no estimates)".to_string();
    }
    let mut sorted = ratios.to_vec();
    sorted.sort_by(|x, y| x.partial_cmp(y).expect("ratio of two positive durations"));
    format!(
        "median {:.2} over {} points, min {:.2}, max {:.2}",
        sorted[sorted.len() / 2],
        sorted.len(),
        sorted[0],
        sorted[sorted.len() - 1]
    )
}

/// The id criterion printed: group + bench id, so a ratio row and a `time:` line are the same string.
fn full_id(dirtied: Dirtied, bench_id: &str) -> String {
    format!("{dirtied_group}/{bench_id}", dirtied_group = dirtied.group())
}
