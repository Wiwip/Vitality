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
#[path = "support/leaf.rs"]
mod leaf;

use crate::common::{HarnessWorld, WorldSpec, actor_entities, mark_dirty_fraction, seed_rng, tick};
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

/// Every bench group hangs off here.
fn resolver_benches(c: &mut Criterion) {
    app_update(c);
    leaf_access_paths(c);
    leaf_exclusion_length(c);
    leaf_warm_cold(c);
}

criterion_group!(benches, resolver_benches);
criterion_main!(benches);
