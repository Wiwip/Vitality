//! Criterion entry point for the vitality resolver benches (`.auto/BENCH-PLAN.md`).
//!
//! Only this one bench target exists (`[[bench]] name = "bench_systems", harness = false`), and
//! cargo turns any *other* top-level `benches/*.rs` into a `harness = true` target that cannot
//! link. So shared code lives in `benches/support/` and is pulled in below with `#[path]`: add a
//! group to `resolver_benches`, do not add a file next to this one.
//!
//! Running this in the sandbox. The repo's manifests, lockfile and `~/.cargo/registry` are read-only
//! and there is no network, which is why every cargo call below carries `--offline --locked` and runs
//! in the tree itself: a read-only `Cargo.toml`/`Cargo.lock` costs `--locked` nothing, and the shared
//! `sccache` keeps this profile warm without a mirror or a second `CARGO_HOME`. If `--locked` ever
//! complains the lockfile is stale, which is something to report, never a reason to drop the flag. The
//! gate never compiles bench targets, so compiling them has to be asked for:
//!
//! ```text
//! cd /mnt/rust/RustroverProjects/vitality
//! bash scripts/gate.sh                                       # tests only, no bench targets
//! cargo bench --offline --locked -p vitality --bench bench_systems --no-run   # compile the bench targets (links)
//! cargo bench --offline --locked -p vitality --bench bench_systems   # full run
//! cargo bench --offline --locked -p vitality --bench bench_systems -- scenario \
//!     --warm-up-time 0.5 --measurement-time 1 --sample-size 10       # short run for the log
//! ```
//!
//! Exactly one `--` in that last line (D-35): everything after it goes to criterion, so a second one is
//! what turns a filtered run into the whole suite. Keep the load discipline `.auto/LOG.md` records —
//! read `/proc/loadavg` before and after the run, and report anything under ~10 % as directional
//! (D-36).
//!
//! A bench id is `<group>/<WorldSpec::shape_id()>`, so a number never travels without the shape it
//! came from.
//!
//! Build labels come from `common::build_label`. Plain `cargo bench` is `st-static`; add
//! `--features multi_threaded` for `mt-static`, and `--features dyn` only to reproduce pre-2026-11-10
//! numbers, which were all `mt-dyn` (D-22).

#[path = "support/common.rs"]
mod common;
#[path = "support/expr.rs"]
mod expr;
#[path = "support/scenario.rs"]
mod scenario;

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

/// Every bench group hangs off here.
/// Game-shaped ticks at 1k/5k/10k actors (`support/scenario.rs`). Owner's budget at 10k actors:
/// `steady` under 2 ms, `storm` under 8 ms. Set `VITALITY_SCENARIO_ACTORS=10000` to run one size.
fn scenario_ticks(c: &mut Criterion) {
    let sizes: Vec<usize> = match std::env::var("VITALITY_SCENARIO_ACTORS") {
        Ok(list) => list
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect(),
        Err(_) => vec![1_000, 5_000, 10_000],
    };
    let mut group = c.benchmark_group("scenario");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_millis(500));
    group.measurement_time(std::time::Duration::from_secs(3));
    for &actors in &sizes {
        for workload in scenario::Workload::ALL {
            let id = scenario::scenario_id(workload, actors);
            let mut world = scenario::build(workload, actors);
            group.bench_function(id.trim_start_matches("scenario/"), |b| {
                b.iter_custom(|iters| {
                    let mut total = std::time::Duration::ZERO;
                    for _ in 0..iters {
                        world.drive();
                        let start = std::time::Instant::now();
                        world.tick();
                        total += start.elapsed();
                    }
                    total
                })
            });
        }
    }
    group.finish();
}

/// P-12, BENCH-PLAN §2C `leaf/expr_depth` + `leaf/typed_vs_hashmap`: one expression evaluated
/// through `Arc<dyn Expr>`, through its concrete type, and as typemap lookups. Differences under
/// ~10 % are noise on this box (D-36). Results: `.auto/BENCH-RESULTS.md`, P-12 table.
fn leaf_expr_shape(c: &mut Criterion) {
    let fixture = expr::expr_fixture();
    let mut group = c.benchmark_group("leaf/expr_shape");
    for (nodes, erased) in &fixture.erased {
        let nodes = *nodes;
        let label = common::build_label();
        group.bench_function(format!("erased/nodes={nodes}/{label}"), |b| {
            b.iter(|| black_box(expr::eval_erased(&fixture, erased)))
        });
        group.bench_function(format!("mono/nodes={nodes}/{label}"), |b| {
            b.iter(|| black_box(expr::eval_mono(&fixture, black_box(nodes))))
        });
        group.bench_function(format!("typemap/nodes={nodes}/{label}"), |b| {
            b.iter(|| black_box(expr::eval_typemap(&fixture, black_box(nodes))))
        });
    }
    group.finish();
}

fn resolver_benches(c: &mut Criterion) {
    scenario_ticks(c);
    leaf_expr_shape(c);
}

criterion_group!(benches, resolver_benches);
criterion_main!(benches);
