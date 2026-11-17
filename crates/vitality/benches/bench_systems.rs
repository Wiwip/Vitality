//! The only bench target (`[[bench]] name = "bench_systems", harness = false`): cargo turns any other
//! top-level `benches/*.rs` into a `harness = true` target that cannot link, so shared code lives in
//! `benches/support/`, pulled in below with `#[path]`. Add a group to `resolver_benches`, never a file
//! next to this one, and remember the gate compiles tests and never bench targets.
//!
//! A bench id is `<group>/<WorldSpec::shape_id()>`, so a number never travels without the shape it came
//! from, and `common::build_label` says which feature build measured it.

#[path = "support/common.rs"]
mod common;
#[path = "support/expr.rs"]
mod expr;
#[path = "support/scenario.rs"]
mod scenario;

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

/// Game-shaped ticks at 1k/5k/10k actors; `VITALITY_SCENARIO_ACTORS` replaces the sizes.
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

/// One expression evaluated through `Arc<dyn Expr>`, through its concrete type, and as typemap lookups.
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
