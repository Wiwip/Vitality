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
//! came from. Label each number `st`/`mt` and `dyn`/`static` by hand when it is quoted: this crate's
//! dev-dependencies turn on bevy's `dynamic_linking` and leave `multi_threaded` off (BENCH-PLAN §1.4).

#[path = "support/common.rs"]
mod common;
#[path = "support/floor.rs"]
mod floor;
#[path = "support/leaf.rs"]
mod leaf;
#[path = "support/recompute.rs"]
mod recompute;

use crate::common::{
    HarnessWorld, WorldSpec, actor_entities, mark_dirty_fraction, run_fixed_tick, seed_rng, tick,
};
use crate::leaf::{
    Arity, Excl0, Excl5, Excl20, Excl40, ExclFixture, LeafFixture, excl_fixture, leaf_bench_spec,
    leaf_fixture, leaf_sweep_spec, leaf_view, read_by_id, read_entity_ref, read_except, read_slot,
};
use bevy::ecs::bundle::Bundle;
use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main,
};
use std::hint::black_box;

/// Reality check: a whole `app.update()`, which under `TimeUpdateStrategy::FixedTimesteps(1)` is
/// exactly one fixed tick of the real schedule set-up.
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
        // Build and re-marking are setup, so only the tick is timed and the dirty set cannot
        // drift across samples (BENCH-PLAN §1.3 and §7). `PerIteration`: an `App` is not a value
        // to keep several of in memory.
        b.iter_batched_ref(
            || {
                let mut app = spec.build_app();
                let attrs = app.world().resource::<HarnessWorld>().attrs();
                let actors = actor_entities(app.world_mut());
                let mut rng = seed_rng();
                mark_dirty_fraction(
                    app.world_mut(),
                    attrs,
                    &actors,
                    spec.dirty_fraction,
                    &mut rng,
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
/// Measured at this tip, `st`/`dyn` (bevy's `multi_threaded` off, `libbevy_dylib.so` linked),
/// `/proc/loadavg` 3.47 before the run and 3.06 after, 1000 actors each:
///
/// ```text
/// a=1    21 077 ns/tick     a=10    49 347 ns/tick    a=20    83 953 ns/tick
/// a=40  153 218 ns/tick     a=80   338 551 ns/tick
/// slope 4 033 ns per attribute per tick, intercept 7 436 ns/tick, R^2 0.9934
/// ```
///
/// That slope is 2× BENCH-PLAN §3's 2 µs/attr/tick row, so the row's decision stands: collapse the
/// systems behind an erased op table regardless of what `recompute/all_attrs` says. Two readings of
/// the same five points matter for whoever acts on it. The marginal cost is convex — 3.1 µs/attr
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
/// tip (`st`/`dyn`, for both
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

/// Every bench group hangs off here.
fn resolver_benches(c: &mut Criterion) {
    app_update(c);
    floor_idle_tick(c);
    leaf_access_paths(c);
    leaf_exclusion_length(c);
    leaf_warm_cold(c);
    recompute_ratio(c);
}

criterion_group!(benches, resolver_benches);
criterion_main!(benches);
