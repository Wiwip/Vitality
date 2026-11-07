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

use crate::common::{HarnessWorld, WorldSpec, actor_entities, mark_dirty_fraction, seed_rng, tick};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

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

/// Every bench group hangs off here.
fn resolver_benches(c: &mut Criterion) {
    app_update(c);
}

criterion_group!(benches, resolver_benches);
criterion_main!(benches);
