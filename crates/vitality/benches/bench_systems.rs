//! Criterion entry point for the vitality resolver benches (BENCH-PLAN.md).
//!
//! Only this one bench target exists (`[[bench]] name = "bench_systems", harness = false`), and
//! cargo turns any *other* top-level `benches/*.rs` into a `harness = true` target that cannot
//! link. So shared code lives in `benches/support/` and is pulled in below with `#[path]`: add a
//! group to `resolver_benches`, do not add a file next to this one.
//!
//! Running this in the sandbox. The repo's manifests, lockfile and `~/.cargo/registry` are
//! read-only and there is no network, so cargo only runs inside the `/tmp` mirror; the gate never
//! compiles bench targets, so `--no-run` has to be asked for:
//!
//! ```text
//! export VITALITY_MIRROR=/tmp/vitality-auto VITALITY_CARGO_HOME=/tmp/ch-auto
//! cd /mnt/rust/RustroverProjects/vitality
//! bash scripts/gate.sh                                     # tests only, no bench targets
//! cd /tmp/vitality-auto && cargo bench --offline --no-run   # compile the bench targets
//! cd /tmp/vitality-auto && cargo bench --offline            # full run
//! cd /tmp/vitality-auto && cargo bench --offline -- --warm-up-time 0.5 --measurement-time 1 \
//!     --sample-size 10                                      # short run for the log
//! ```
//!
//! A bench id is `<group>/<WorldSpec::shape_id()>`, so a number never travels without the shape it
//! came from.
//!
//! **Every number this repo can produce is an `mt`/`dyn` build, never `st`/`static`.** In the mirror,
//! `cargo tree --offline -e features -p vitality | grep -c multi_threaded` prints **12** and
//! `grep -c dynamic_linking` prints **3**: the `[dev-dependencies.bevy]` block of
//! `crates/vitality/Cargo.toml` (features `ui`, `dynamic_linking`, `debug`, …) unifies with the
//! library's bevy for every test and bench target, and `ui` → `default_platform` → `multi_threaded` →
//! `bevy_ecs`/`bevy_tasks`. A `st` label therefore describes a build nobody measured, and BENCH-PLAN
//! §3's floor row — keyed on `st`, `static` — cannot be satisfied or dismissed from this tree as it
//! stands; see [`floor_idle_tick`] for what that does to the decision and `.auto/decisions.md` D-22
//! for the manifest change (protected path, owner's call) that would settle it.

/// The counting allocator of `allocs_per_tick` below. Its `#[global_allocator]` static is declared
/// here rather than in the module, so each binary that `#[path]`-includes it — this one and
/// `tests/test_bench_allocations.rs` — declares exactly one, for itself.
#[path = "support/alloc_count.rs"]
mod alloc_count;
#[path = "support/common.rs"]
mod common;
#[path = "support/cross_actor.rs"]
mod cross_actor;
#[path = "support/floor.rs"]
mod floor;
// Declared, not called: no group staggers yet, and rule 2 of tests/test_bench_wiring.rs needs every
// support module wired here so the gate compiles it.
#[path = "support/identity.rs"]
mod identity;
#[path = "support/leaf.rs"]
mod leaf;
#[path = "support/recompute.rs"]
mod recompute;

use crate::alloc_count::{Calibration, Counts};
use crate::common::{
    Attr0, Attr4, Attr19, AttrSpec, HarnessWorld, WorldSpec, actor_entities, mark_dirty_fraction,
    run_fixed_tick, seed_rng, tick,
};
use crate::cross_actor::{
    SourceMove, cascade_tick, cross_id, cross_shape, cross_spec, passes_line, passes_to_converge,
};
use crate::leaf::{
    Arity, Excl0, Excl5, Excl20, Excl40, ExclFixture, LeafFixture, excl_fixture, leaf_bench_spec,
    leaf_fixture, leaf_sweep_spec, leaf_view, read_by_id, read_entity_ref, read_except, read_slot,
};
use bevy::ecs::bundle::Bundle;
use bevy::prelude::{App, Entity, World};
use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
};
use std::hint::black_box;
use vitality::attributes::Attribute;

/// Reality check: a whole `app.update()`, which under `TimeUpdateStrategy::FixedTimesteps(1)` is
/// exactly one fixed tick of the real schedule set-up.
///
/// The id carries `dirty=25`, so the timed tick has to start on a world dirty on 25 of its 100 actors
/// and clean on the other 75. It did not: `build_app_with` hands back a world dirty on *every* actor
/// (`common.rs`), the re-mark below only added marks to marks, and before P-13 the marks it added were
/// still deferred, so every sample timed a 100 %-dirty tick under a `dirty=25` label. The setup now
/// settles the world first — [`floor::prepare_floor`]: tick, clear every marker, one idle
/// `run_fixed_tick` that has to leave nothing dirty — and marks the fraction afterwards, which
/// [`mark_dirty_fraction`] now flushes so the marks are really on those entities when `tick` runs.
/// The count is asserted per sample, in setup, because an id nobody checks is a label.
///
/// Build and settling are setup, not measurement, so only the tick is timed and the dirty set cannot
/// drift across samples (BENCH-PLAN §1.3 and §7). `PerIteration`: an `App` is not a value to keep
/// several of in memory.
fn app_update(c: &mut Criterion) {
    let spec = WorldSpec {
        attributes_per_actor: 8,
        effects_per_actor: 4,
        modifiers_per_effect: 1,
        actors: 100,
        dirty_fraction: 0.25,
        ticking_fraction: 0.0,
    };

    let mut group = c.benchmark_group("harness/app_update");
    group.throughput(Throughput::Elements(spec.actors as u64));
    group.bench_function(spec.shape_id(), |b| {
        b.iter_batched_ref(
            || {
                let mut app = spec.build_app();
                // Clean before the fraction, or `dirty=25` describes nothing: a built world is dirty on
                // everything and a resolve re-marks itself, so ticking is not how you get there.
                floor::prepare_floor(&mut app);
                let attrs = app.world().resource::<HarnessWorld>().attrs();
                let actors = actor_entities(app.world_mut());
                let mut rng = seed_rng();
                let marked =
                    mark_dirty_fraction(app.world_mut(), attrs, &actors, spec.dirty_fraction, &mut rng);
                assert_eq!(
                    marked,
                    (spec.actors as f32 * spec.dirty_fraction).round() as usize,
                    "shape {} asked for a {} fraction of {} actors and got {marked}",
                    spec.shape_id(),
                    spec.dirty_fraction,
                    spec.actors
                );
                assert_eq!(
                    floor::dirty_entity_count(app.world()),
                    marked,
                    "shape {} marks {marked} actors in its setup but its world carries {}: the id's \
                     dirty= field is not the workload the timed tick sees",
                    spec.shape_id(),
                    floor::dirty_entity_count(app.world())
                );
                app
            },
            tick,
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

/// Family A (BENCH-PLAN §2A): what one registered attribute costs a tick that has nothing to do.
///
/// An attribute is not only a component. `init_attribute` puts six systems into `FixedUpdate` for
/// its type, and with no dirty actor every one of them still runs, prepares its archetypes and
/// iterates nothing. Five points on one axis — 1/10/20 attributes from the harness's default pool,
/// 40/80 from [`floor::FLOOR_POOL`] — over 1000 actors each, so the y-axis is only the schedule.
///
/// The world is built *before* `b.iter` and never rebuilt: a floor is by definition a hot-loop
/// number, and `leaf/warm_cold` showed what a per-sample build does to one (`iter_batched_ref`
/// setup is milliseconds of allocation that the reported ns never includes, but which is exactly what
/// evicts the caches the tick would run on). The same world is ticked hundreds of times here, which
/// is only sound because it stays idle — [`floor::prepare_floor`] clears the dirty markers and
/// asserts one idle tick later that nothing re-marked itself, and the assert below re-checks that
/// after the loop. Ticking alone does not reach that state: a resolve hands out `&mut T`, which
/// `EffectsSet::Notify` reads as a change and answers with a fresh `MarkNodeDirty`, so a world with
/// markers left in it re-resolves every attribute of every actor forever.
///
/// `run_fixed_tick` is `World::run_schedule(FixedUpdate)`, not `app.update()`: it runs the resolver
/// and nothing else. It also does not advance `Time<Fixed>` (`common.rs`), which is harmless for a
/// shape with zero effects and is what keeps this a floor — do not "fix" the time here, or the group
/// quietly becomes `periodic/*`.
///
/// The metric is ns/tick, so `Throughput` is the attribute count: an idle tick touches no actor, and
/// an `Elements(actors)` line would price 1000 entities the tick never looks at. Criterion reports
/// each point; the ns-per-attribute-per-tick slope §3 reads is the least-squares fit over these five
/// ids, from the `mean.point_estimate` in each `target/criterion/floor_idle_tick/<id>/new/estimates.json`
/// the run writes (all five in one run, so the fit is the same measurement the ids print).
///
/// Measured at this tip, `mt`/`dyn` (`multi_threaded` on and `libbevy_dylib.so` linked, as the module
/// doc says; the label here read `st` until P-13 — the numbers below are that same run, only the label
/// was wrong), `/proc/loadavg` 3.47 before the run and 3.06 after, 1000 actors each:
///
/// ```text
/// a=1    21 077 ns/tick     a=10    49 347 ns/tick    a=20    83 953 ns/tick
/// a=40  153 218 ns/tick     a=80   338 551 ns/tick
/// slope 4 033 ns per attribute per tick, intercept 7 436 ns/tick, R^2 0.9934
/// ```
///
/// That slope is 2× BENCH-PLAN §3's 2 µs/attr/tick row, so the row's decision — collapse the systems
/// behind an erased op table regardless of what `recompute/all_attrs` says — reads as met. Read it with
/// the caveat: the row is keyed on `st`/`static` (`BENCH-PLAN.md:146`), a build this crate cannot
/// produce without a manifest edit (the dev-dependency bevy turns `multi_threaded` and
/// `dynamic_linking` on for every bench, module doc above), so that 2× is one `mt`/`dyn` measurement
/// held against a threshold set for a build nobody can measure here. D-22: the direction of the
/// correction is not guessable, because a single-threaded executor removes work-stealing *and* any
/// parallel execution of the six per-attribute systems. Two readings of the same five points matter
/// for whoever acts on it. The marginal cost is convex — 3.1 µs/attr
/// between a=1 and a=10, 3.5 across the middle, 4.6 µs/attr between a=40 and a=80 — so the fit
/// understates an attribute at the top of the range. And the 7.4 µs intercept is fitted, not
/// measured: every shape registers one more attribute than its id counts, because `AttributesPlugin`
/// registers `Stacks` into every app, so roughly one attribute's worth of that intercept is attribute
/// machinery and only the remainder is the `FixedUpdate` walk itself. `floor_spec(0, 1000)` is the
/// point that would split the two.
fn floor_idle_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("floor/idle_tick");
    for attributes in floor::FLOOR_COUNTS {
        let spec = floor::floor_spec(attributes, floor::FLOOR_ACTORS);
        let mut shape = floor::floor_shape(&spec, floor::floor_pool(attributes));
        assert_eq!(
            shape.app.world().resource::<HarnessWorld>().attrs().len(),
            attributes,
            "shape {} registered a different number of attributes than it declares",
            spec.shape_id()
        );
        group.throughput(Throughput::Elements(attributes as u64));
        let world = shape.app.world_mut();
        group.bench_function(format!("{}/run_fixed_tick", spec.shape_id()), |b| {
            b.iter(|| {
                run_fixed_tick(world);
                // One read of the world the tick just ran on, so the result is consumed.
                black_box(world.entities().len());
            });
        });
        assert_eq!(
            floor::dirty_entity_count(world),
            0,
            "shape {} ended its timed loop with {} dirty entities: the samples after the first one \
             were resolving, not idling",
            spec.shape_id(),
            floor::dirty_entity_count(world)
        );
    }
    group.finish();
}

/// Family C (BENCH-PLAN §2C): one attribute leaf read four ways, on the same generated world.
/// Metric is ns/read, so `Throughput` stays unset.
///
/// Setup is [`leaf_fixture`]: build the world, tick it once so the resolver has actually run,
/// snapshot the slot blocks, resolve the `ComponentId` and the view `QueryState`. Measured is one
/// `read_*` call per iteration — for path (i) that is
/// `QueryState::get` (the per-actor handle fetch the library does before it touches any leaf) plus
/// the typed `get` whose bundle scan is what T3 blames, so (i) is an upper bound on the scan alone.
fn leaf_access_paths(c: &mut Criterion) {
    let spec = leaf_bench_spec();

    // The exclusion width goes in the id because it is the variable `leaf/exclusion_length` sweeps,
    // and `a=8` does not reveal it. Carried on all four ids so the (i)/(iii) ratio that decides
    // BENCH-PLAN §3 stays a pair of ids from one run.
    let excl = leaf_fixture(&spec).excl;

    let mut group = c.benchmark_group("leaf/access_paths");
    group.bench_function(
        format!("{}/path_i_except_get/excl={}", spec.shape_id(), excl),
        |b| {
            b.iter_batched_ref(
                || leaf_fixture(&spec),
                |fixture: &mut LeafFixture| black_box(read_leaf(fixture)),
                BatchSize::PerIteration,
            );
        },
    );
    group.bench_function(
        format!("{}/path_ii_entity_get/excl={}", spec.shape_id(), excl),
        |b| {
            b.iter_batched_ref(
                || leaf_fixture(&spec),
                |fixture: &mut LeafFixture| {
                    black_box(read_entity_ref(fixture.app.world(), fixture.actor))
                },
                BatchSize::PerIteration,
            );
        },
    );
    group.bench_function(
        format!("{}/path_iii_get_by_id/excl={}", spec.shape_id(), excl),
        |b| {
            b.iter_batched_ref(
                || leaf_fixture(&spec),
                |fixture: &mut LeafFixture| {
                    black_box(read_by_id(fixture.app.world(), fixture.actor, fixture.id))
                },
                BatchSize::PerIteration,
            );
        },
    );
    group.bench_function(
        format!("{}/path_iv_slot_block/excl={}", spec.shape_id(), excl),
        |b| {
            b.iter_batched_ref(
                || leaf_fixture(&spec),
                |fixture: &mut LeafFixture| {
                    black_box(read_slot(fixture.app.world(), fixture.actor))
                },
                BatchSize::PerIteration,
            );
        },
    );
    group.finish();
}

/// Family C (BENCH-PLAN §2C): what one component in the exclusion bundle costs. Same leaf read, in
/// `EntityRefExcept<B>` for bundles of 0, 5, 20 and 40 throwaway marker components, so the slope of
/// ns/read against the width is the price of `bundle_contains_component` walking one more type — and
/// the number "shrink the exclusion bundle" has to be argued from.
///
/// Cold, i.e. an `App` build per sample, the way `leaf/access_paths` measures, so a width's cost is
/// the same kind of number as (i)'s — though the sweep's shape carries `m=2` (see
/// [`leaf_sweep_spec`]), so read the *slope* across widths rather than comparing one absolute with
/// P-2's (i). Ids carry both `excl=` (members this world has a `ComponentId` for) and `arity=`
/// (members the tuple has): the two only agree because building the view registers the markers, and
/// if that ever stops being true the sweep's own x-axis is wrong, which `exclusion_length_reads_agree`
/// in the gate now fails on. Its warm floor ships next door as
/// `leaf/warm_cold/…/warm_except_get/excl=0/arity=0`, so the slope has a warm counterpart to be read
/// against.
fn leaf_exclusion_length(c: &mut Criterion) {
    let spec = leaf_sweep_spec();
    let mut group = c.benchmark_group("leaf/exclusion_length");
    excl_width::<Excl0>(&mut group, &spec);
    excl_width::<Excl5>(&mut group, &spec);
    excl_width::<Excl20>(&mut group, &spec);
    excl_width::<Excl40>(&mut group, &spec);
    group.finish();
}

/// One width: the view fetch plus one typed `get`, which is exactly what path (i) times.
fn excl_width<B: Bundle + Arity>(group: &mut BenchmarkGroup<'_, WallTime>, spec: &WorldSpec) {
    // One throwaway build, only to put the width the id claims next to the width the tuple has.
    let probe = excl_fixture::<B>(spec);
    group.bench_function(
        format!(
            "{}/except_get/excl={}/arity={}",
            spec.shape_id(),
            probe.excl,
            B::ARITY
        ),
        |b| {
            b.iter_batched_ref(
                || excl_fixture::<B>(spec),
                |fixture: &mut ExclFixture<B>| black_box(read_excl(fixture)),
                BatchSize::PerIteration,
            );
        },
    );
}

/// Family C: whether the ratio §3 keys on is a cache-miss number, and what the exclusion scan costs
/// once nothing else is being re-warmed.
///
/// `cold` repeats `leaf/access_paths/path_i_except_get`: `iter_batched_ref` setup is a whole `App`
/// build plus a tick per sample — milliseconds of allocation the reported number does not include,
/// but which is exactly what evicts the caches around a read that has measured 572–695 ns at this
/// tip (`mt`/`dyn`, for both
/// `leaf/access_paths/a=8/e=2/m=1/actors=10/dirty=0/tick=0/path_i_except_get/excl=9` and its
/// `leaf/warm_cold/a=8/e=2/m=1/actors=10/dirty=0/tick=0/cold_except_get/excl=9` twin). `warm` is one
/// already-built fixture read back to back, so nothing evicts anything.
///
/// The rows this feeds are ratios, not levels: BENCH-PLAN §3 fixes leaf access first when
/// `leaf/access_paths` gives (i) ≥ 3× (iii), and calls `(i)/(iii)` in (1×, 3×) ambiguous with only
/// the cheap exclusion-bundle shrink on offer; §2C prices that shrink as ns against exclusion-list
/// length, with no µs threshold attached. `warm_except_get/excl=0/arity=0` is the zero-width
/// reference — the same read through a bundle with nothing to scan — so the warm price of a narrower
/// bundle is a subtraction inside this group instead of an inference from the cold column.
fn leaf_warm_cold(c: &mut Criterion) {
    // Same shape, same read and same setup as `leaf/access_paths/path_i_except_get`, so the cold
    // column here is that id and the ratio is between two runs of one thing.
    let spec = leaf_bench_spec();
    let excl = leaf_fixture(&spec).excl;

    let mut group = c.benchmark_group("leaf/warm_cold");
    group.bench_function(
        format!("{}/cold_except_get/excl={}", spec.shape_id(), excl),
        |b| {
            b.iter_batched_ref(
                || leaf_fixture(&spec),
                |fixture: &mut LeafFixture| black_box(read_leaf(fixture)),
                BatchSize::PerIteration,
            );
        },
    );
    group.bench_function(
        format!("{}/warm_except_get/excl={}", spec.shape_id(), excl),
        |b| {
            // Built before the timed loop, so criterion measures the read alone and every sample
            // after the first finds the same storage hot.
            let mut fixture = leaf_fixture(&spec);
            b.iter(|| black_box(read_leaf(&mut fixture)));
        },
    );

    // The floor of the width sweep, warm, so 9-wide-vs-zero is two numbers from one group. On the
    // sweep's shape (one more permanent modifier per effect than the two ids above) because that is
    // the world `leaf/exclusion_length` measures, and its slope is what this reference is for.
    let sweep_spec = leaf_sweep_spec();
    let probe = excl_fixture::<Excl0>(&sweep_spec);
    group.bench_function(
        format!(
            "{}/warm_except_get/excl={}/arity={}",
            sweep_spec.shape_id(),
            probe.excl,
            <Excl0 as Arity>::ARITY
        ),
        |b| {
            let mut fixture = excl_fixture::<Excl0>(&sweep_spec);
            b.iter(|| black_box(read_excl(&mut fixture)));
        },
    );
    group.finish();
}

/// Path (i)'s two calls on the library's own view bundle: the per-actor view fetch, then the typed
/// `get` whose bundle scan is the thing under test.
fn read_leaf(fixture: &mut LeafFixture) -> f64 {
    let view = leaf_view(fixture.app.world(), &mut fixture.view, fixture.actor);
    read_except(&view)
}

/// The same two calls through a sweep bundle.
fn read_excl<B: Bundle>(fixture: &mut ExclFixture<B>) -> f64 {
    let view = leaf_view(fixture.app.world(), &mut fixture.view, fixture.actor);
    read_except(&view)
}

/// Family B (BENCH-PLAN §2B): does the effect walk run once per attribute (T2)?
///
/// One actor, A attributes, E permanent effects x 1 modifier, `b.iter` over [`run_fixed_tick`] on a
/// world built outside the closure. The only thing `one_attr` and `all_attrs` differ in is how many
/// of the A attributes carry a `Dirty<T>`, and the deliverable is the ratio of the two, which
/// [`recompute::print_ratios`] prints per A from the `estimates.json` criterion writes for these very
/// ids (criterion 0.8 stores each benchmark as it finishes, so the `one_attr` file is on disk before
/// the `all_attrs` one of the same A is read).
///
/// What each side really times, per the asserts in `support/recompute.rs` and
/// `test_recompute_ratio`: a resolve re-marks itself (`update_attribute` takes `&mut T`,
/// `on_change_notify_attribute_parents::<T>` in `EffectsSet::Notify` answers that with a fresh
/// `MarkNodeDirty`), so `recompute/one_attr` is *one attribute permanently dirty* — the same
/// attribute, the same actor, every sample — and `recompute/all_attrs` is the realistic steady state
/// of this design rather than a worst case. The ids say `dirty=one` / `dirty=all`, and the assert
/// after each timed loop re-reads the dirty set per attribute so a shape cannot drift into the other
/// one's workload mid-run.
///
/// `Throughput` stays unset: the metric is ns/tick of one actor, and `Elements(A)` would invite a
/// reading of these as ns/attribute, which is what the ratio exists to establish.
fn recompute_ratio(c: &mut Criterion) {
    for attributes in recompute::RECOMPUTE_ATTR_COUNTS {
        let mut none = Vec::new();
        let mut one = Vec::new();
        let mut all = Vec::new();
        // One side entirely before the other, so both estimate files of an A exist when its ratio
        // prints, and so the shapes of one side are built off the same cache state as each other.
        for dirtied in [
            recompute::Dirtied::None,
            recompute::Dirtied::One,
            recompute::Dirtied::All,
        ] {
            let mut group = c.benchmark_group(dirtied.group());
            for effects in recompute::RECOMPUTE_EFFECT_COUNTS {
                let spec = recompute::recompute_spec(attributes, effects);
                let bench_id = format!("{}/run_fixed_tick", recompute::dirtied_id(&spec, dirtied));
                let mut shape = recompute::recompute_shape(&spec, dirtied);
                let pool = shape.app.world().resource::<HarnessWorld>().attrs();
                let actor = shape.actor;
                let world = shape.app.world_mut();
                group.bench_function(&bench_id, |b| {
                    b.iter(|| {
                        run_fixed_tick(world);
                        black_box(world.entities().len());
                    });
                });
                recompute::assert_dirty_attrs(
                    world,
                    pool,
                    actor,
                    dirtied,
                    &format!("{bench_id} after its timed loop"),
                );
                let side = recompute::Point { spec, bench_id };
                match dirtied {
                    recompute::Dirtied::None => none.push(side),
                    recompute::Dirtied::One => one.push(side),
                    recompute::Dirtied::All => all.push(side),
                }
            }
            group.finish();
        }
        recompute::print_ratios(attributes, &none, &one, &all);
    }
}

/// The one place a grid point's attribute types and the pool slot names they must sit in are written
/// together, so the `stringify!` [`cross_shape`] asserts with cannot drift from the types it builds with.
macro_rules! cross_actor_point {
    ($group:expr, $attributes:expr, $readers:expr, $src:ty, $dep:ty) => {
        cross_actor_point::<$src, $dep>(
            $group,
            $attributes,
            $readers,
            stringify!($src),
            stringify!($dep),
        )
    };
}

/// Family B (BENCH-PLAN §2B): the cross-actor cascade — what one dirty source costs the K actors whose
/// modifiers read it.
///
/// Nine points: the same A axis `recompute/one_attr` uses, times K ∈ {1, 10, 100} readers. Each reader
/// carries one permanent effect whose single modifier is `EffectSubject::Source` and reads the source's
/// attribute, applied with the `ApplyEffectEvent` / `EffectTargeting::new(source, target)` pair
/// `Vitality::apply_effect_to_target` triggers, so the timed tick is the fan-out — `AttributeDependents`
/// → `RecalculateExpression` → `MarkNodeDirty` up `EffectTarget` — rather than a fold. The shape, what the
/// timed unit contains and why a bare mark cannot be it are all in [`cross_actor`]; the gate guard is
/// `tests/test_bench_cross_actor.rs`.
///
/// Ids carry `readers=K` and `cross=src-write` because `WorldSpec` has no field for the second actor, the
/// cross effects, or the write inside the timed unit — and adding one would break the full `WorldSpec
/// { .. }` literals in `tests/test_bench_harness.rs`. As with the leaf family's `excl=`, a shape the id
/// cannot say is a label.
///
/// Each point prints its [`cross_actor::passes_to_converge`] line before timing: §2B reads that count as
/// the staleness half of the row, and it is measured on the world the µs/tick beside it comes from. Both
/// counts came out > 1, so §2B's per-tick staleness is confirmed and µs/tick is not a latency: a plain
/// source write takes **3 resolver ticks** to reach a reader, 2 once `MarkNodeDirty` is triggered by hand
/// — the write is only marked in `EffectsSet::Notify`, which runs after `UpdateCurrentValues`, and the
/// readers are only marked during a resolve, so each of those two steps costs a tick.
///
/// Measured at this tip, `mt`/`dyn`, mean point estimate, 10 samples per id (`--warm-up-time 0.5
/// --measurement-time 1`), `/proc/loadavg` 4.40 on 8 cores when the run ended — directional per §7, and
/// `std_dev` runs 2.6–32.5 µs, wider on the `readers=100` points:
/// ```text
/// µs/tick      readers=1   readers=10   readers=100
/// a=1            25.4         44.1         194.4
/// a=5            42.2         57.3         217.9
/// a=20           90.9        117.0         329.6
/// ```
///
/// The row is a level plus a per-reader slope, and the slope is the cascade: 169–239 µs between
/// readers=1 and readers=100 — 99 more readers — is **1.7–2.4 µs per reader per tick**, and it is
/// flatter in A than the level is. The level's A-dependence is the schedule's, but read that against
/// `floor/idle_tick` as directional, not as a same-run delta: the floor prices 4.0 µs per attribute per
/// tick on an idle shape and at readers=1 this row sits only 4–7 µs above it (25.4 vs 21.1 at a=1, 90.9
/// vs 84.0 at a=20), yet this `target/criterion` holds only `recompute_cross_actor` — those two floor
/// points come from an earlier run — and σ on the readers=1 points here is 2.6–6.5 µs, the same order
/// as the gap. What it does say is that a cascade of one reader is small next to the walk it rides on,
/// and the readers are the cost. 100 readers behind a source that moves every tick is 0.33 ms at the
/// widest shape — 2 % of a 60 Hz frame, for derived stats that arrive 3 ticks late.
///
/// `Throughput` stays unset: the metric is µs/tick of one source and its readers, and an `Elements(K)`
/// line would price readers a tick only reaches through the cascade.
fn recompute_cross_actor(c: &mut Criterion) {
    let mut group = c.benchmark_group("recompute/cross_actor");
    for readers in cross_actor::CROSS_READERS {
        // The dependent modifier sits on the shape's last attribute, so `a=1` is the point where the
        // attribute read and the attribute written are one type, and `a=20` is the widest the default
        // pool serves. `cross_shape` fails if a slot is not the type named beside it.
        cross_actor_point!(&mut group, 1, readers, Attr0, Attr0);
        cross_actor_point!(&mut group, 5, readers, Attr0, Attr4);
        cross_actor_point!(&mut group, 20, readers, Attr0, Attr19);
    }
    group.finish();
}

/// One point: build settled, print the pass count, settle again, then time [`cascade_tick`] — one source
/// write plus one [`run_fixed_tick`] — on that world. Built outside the timed loop like every other §2B
/// group, because the row is a steady-state per-tick number and a per-sample build would price the
/// allocator instead of the cascade.
fn cross_actor_point<S, D>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    attributes: usize,
    readers: usize,
    src_name: &str,
    dep_name: &str,
) where
    S: Attribute<Property = f32>,
    D: Attribute<Property = f32>,
{
    let spec = cross_spec(attributes, readers);
    let mut shape = cross_shape::<S, D>(&spec, src_name, dep_name);
    let write = passes_to_converge::<S, D>(&mut shape, SourceMove::Write);
    let marked = passes_to_converge::<S, D>(&mut shape, SourceMove::WriteThenMark);
    println!("{}", passes_line(&shape, write, marked));
    // Back to a world at rest: the pass counting leaves every actor dirty, and the first samples would
    // otherwise price a cascade the shape's id does not claim.
    floor::prepare_floor(&mut shape.app);
    let source = shape.source;
    let reader_list = shape.readers.clone();
    let world = shape.app.world_mut();
    let before: Vec<f64> = reader_list
        .iter()
        .map(|&reader| dep_value::<D>(world, reader))
        .collect();

    group.bench_function(cross_id(&spec), |b| {
        b.iter(|| black_box(cascade_tick::<S>(world, source)));
    });

    // A flat reader is the one way this group can quietly time the idle walk. The source only ever
    // moves up, so every reader's dependent value has to move up too.
    for (&reader, &was) in reader_list.iter().zip(&before) {
        let now = dep_value::<D>(world, reader);
        assert!(
            now > was,
            "shape {} ended its timed loop with a reader whose dependent attribute never moved past \
             {was}: no sample carried a source change to it, so the µs/tick above is the idle walk \
             wearing a cross_actor label",
            cross_id(&spec)
        );
    }
}

/// One reader's dependent value, read the way the row is about.
fn dep_value<D: Attribute<Property = f32>>(world: &World, reader: Entity) -> f64 {
    world
        .get::<D>(reader)
        .expect("every reader carries the dependent attribute the shape is typed on")
        .current_value() as f64
}

#[global_allocator]
static ALLOC_COUNT: alloc_count::CountingAlloc = alloc_count::ALLOC;

/// How many ticks one `allocs_per_tick` window runs. Long enough that one tick's allocator warm-up is
/// a small part of the count; short enough that the window is not a bench of its own.
const ALLOC_TICKS: u64 = 50;

/// The share of the actors a group shape marks dirty for its second column.
const ALLOC_DIRTY_FRACTION: f32 = 0.25;

/// The two shapes the group measures, both built by [`WorldSpec::build_app`] and settled by
/// [`floor::prepare_floor`], so P-14 borrows nothing from the leaf or recompute families.
fn alloc_shapes() -> [WorldSpec; 2] {
    [
        // The tiny shape the gate test uses too, so the two files describe one world.
        WorldSpec {
            attributes_per_actor: 3,
            effects_per_actor: 2,
            modifiers_per_effect: 1,
            actors: 4,
            dirty_fraction: 0.0,
            ticking_fraction: 0.0,
        },
        // The leaf family's attribute width at the harness's default actor count, so an allocations
        // line has a duration neighbour of the same shape in `leaf/access_paths`.
        WorldSpec {
            attributes_per_actor: 8,
            effects_per_actor: 2,
            modifiers_per_effect: 1,
            actors: 100,
            dirty_fraction: 0.0,
            ticking_fraction: 0.0,
        },
    ]
}

/// Family G (BENCH-PLAN §2G): allocations per tick, counted by `support/alloc_count.rs`.
///
/// One shape gives two ids — the settled world idling (`dirty=0`) and the same world with
/// [`ALLOC_DIRTY_FRACTION`] of its actors marked through `MarkNodeDirty` (`dirty=25`) — and each id
/// prints an `allocs/tick, bytes/tick` line beside it. The pair is what §3's last unmeasured row is
/// read from: allocs/tick over [`floor::dirty_entity_count`] as "entities touched", which
/// `alloc_count::line` renders against the row's 2-per-entity figure as a lower bound and never a
/// verdict. `Throughput` stays unset because the printed column is the deliverable and an
/// `Elements(actors)` line would price actors an idle tick never looks at.
///
/// **Read the printed lines, not the criterion estimate, for this family.** The estimate is a
/// `WallTime` number — the same [`run_fixed_tick`] the floor and recompute groups time, on the same
/// settled world — and it exists so every allocation line has a duration measured on that very world
/// beside it. It is not a new §3 row: `floor/idle_tick` and `recompute/*` already time this tick.
///
/// ## Which of the two states this run is in, and where to read it
/// Whether this process counts anything is answered by [`alloc_count::calibrate`] at the top of the
/// group and printed once, not assumed here; `support/alloc_count.rs` explains what reaches the wrapper
/// and why a build with `dynamic_linking` on (D-22) is expected to answer "nothing".
///
/// Two readings that hold in either state: the window is whole-process (`multi_threaded`, D-22), so
/// only the *comparison* of the two columns is a claim about the tick — [`alloc_count::pair_line`] prints
/// that comparison and never a total — and installing the wrapper costs one relaxed load per allocation
/// on the off path, which the duration column of every group in this target now pays.
fn allocs_per_tick(c: &mut Criterion) {
    // Once per run: the probe is a property of the process, not of the shape, and every line below is
    // rendered against its answer.
    let cal = alloc_count::calibrate();
    println!("{}", alloc_count::calibration_line(cal));
    let mut group = c.benchmark_group("allocs_per_tick");
    for spec in alloc_shapes() {
        let mut app = spec.build_app();
        // The idle column has to be made, not waited for: a built world is dirty on every attribute of
        // every actor, and a resolve re-marks what it resolved.
        floor::prepare_floor(&mut app);
        let pool = app.world().resource::<HarnessWorld>().attrs();
        let actors = actor_entities(app.world_mut());

        let clean = alloc_column(&mut group, &spec, &mut app, pool, &actors, 0.0, cal);
        let mut dirty_spec = spec.clone();
        dirty_spec.dirty_fraction = ALLOC_DIRTY_FRACTION;
        let dirty = alloc_column(
            &mut group,
            &dirty_spec,
            &mut app,
            pool,
            &actors,
            ALLOC_DIRTY_FRACTION,
            cal,
        );
        println!(
            "{}",
            alloc_count::pair_line(
                &clean.0,
                &dirty.0,
                clean.1,
                dirty.1,
                ALLOC_TICKS,
                dirty.2,
                cal,
            )
        );
    }
    group.finish();
}

/// One column of one shape: the timed tick with the gate closed, then one counted window over the same
/// world with it open, printed beside the same id. Returns (id, counts, entities touched) — a window is
/// read where it is taken, because the counters are process-wide.
fn alloc_column(
    group: &mut BenchmarkGroup<'_, WallTime>,
    spec: &WorldSpec,
    app: &mut App,
    pool: &[AttrSpec],
    actors: &[Entity],
    fraction: f32,
    cal: Calibration,
) -> (String, Counts, usize) {
    let world = app.world_mut();
    let marked = if fraction > 0.0 {
        mark_dirty_fraction(world, pool, actors, fraction, &mut seed_rng())
    } else {
        0
    };
    // Read before the window: a resolving tick re-dirties itself, so the row's denominator is the set
    // the tick starts on, which is also all the id's `dirty=` field claims.
    let touched = floor::dirty_entity_count(world);
    assert_eq!(
        touched,
        marked,
        "shape {} starts an allocation column on {touched} dirty entities while marking {marked}: \
         `dirty_entity_count` is the denominator of §3's row, so an id whose `dirty=` field the world \
         does not carry is a label, not a measurement",
        spec.shape_id()
    );

    let id = format!("{}/run_fixed_tick", spec.shape_id());
    group.bench_function(&id, |b| {
        b.iter(|| {
            run_fixed_tick(world);
            black_box(world.entities().len());
        });
    });

    alloc_count::counting(true);
    let ((), counts) = alloc_count::counted(|| {
        for _ in 0..ALLOC_TICKS {
            run_fixed_tick(world);
        }
    });
    alloc_count::counting(false);
    println!(
        "{}",
        alloc_count::line(&id, counts, ALLOC_TICKS, touched, cal)
    );

    // The guard the other groups run too: a column whose world drifted into the other column's
    // workload reports the wrong thing, and here it would also move the row's denominator.
    let left = floor::dirty_entity_count(world);
    if fraction > 0.0 {
        assert!(
            left > 0,
            "shape {} ended its dirty column on a clean world ({left} dirty entities): the window \
             above was idling, so its line is an idle number wearing a `dirty=` label",
            spec.shape_id()
        );
    } else {
        assert_eq!(
            left,
            0,
            "shape {} ended its idle column with {left} dirty entities: the samples after the first \
             were resolving, and the line printed for it is a dirty number wearing a `dirty=0` label",
            spec.shape_id()
        );
    }
    (id, counts, touched)
}

/// Every bench group hangs off here.
fn resolver_benches(c: &mut Criterion) {
    app_update(c);
    floor_idle_tick(c);
    leaf_access_paths(c);
    leaf_exclusion_length(c);
    leaf_warm_cold(c);
    recompute_ratio(c);
    recompute_cross_actor(c);
    allocs_per_tick(c);
}

criterion_group!(benches, resolver_benches);
criterion_main!(benches);
