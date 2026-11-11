# BENCH-RESULTS

Branch `feat/api-and-resolver`, 2026-11-10/11, 24-core box shared with other work (loadavg 3–10 during
runs). Ratios between rows of one table are the stable signal; absolute values move ~10 % run to
run (D-36). Criterion medians, `--sample-size 10`, 3 s measurement.

Every number is one timed `app.update()`, which is exactly one fixed tick; the per-tick input
(`ScenarioWorld::drive`) runs untimed. Shape for all scenarios: 20 attributes, 3 permanent effects
with one modifier each per actor (`benches/support/scenario.rs`, guarded by
`tests/test_bench_scenario.rs`).

```text
VITALITY_SCENARIOS_ONLY=1 cargo bench --offline --locked -p vitality --bench bench_systems -- scenario
VITALITY_RESOLVER=dag ...            # DAG resolver (the harness defaults to v1)
--features multi_threaded            # mt build
VITALITY_SCENARIO_ACTORS=10000       # one size
```

## Owner's target

10 000 actors: `steady` under 2 ms, `storm` under 8 ms.

## Scenarios at 10 000 actors

| scenario | what moves per tick | v1 st | DAG st | DAG mt | v1 → DAG st |
|---|---|---|---|---|---|
| idle | nothing | 0.25 ms | 0.23 ms | 0.97 ms | 1.1× |
| steady | 10 % of actors, all 20 attributes | 18.0 ms | **1.14 ms** | 1.71 ms | 16× |
| storm | every attribute of every actor | 95.7 ms | 9.63 ms | **6.13 ms** | 10× |
| no_op_write | every attribute rewritten unchanged | 82.0 ms | 0.86 ms | 0.69 ms | 95× |
| damage | 10 % of actors, one attribute | 1.13 ms | 0.33 ms | – | 3.4× |
| aura | 1 caster live-buffing 1000 targets | 1.60 ms | 0.67 ms | – | 2.4× |
| buff_churn | 1000 three-tick buffs applied, as many expire | 22.2 ms | 3.16 ms | 2.73 ms | 7× |
| combat | 2000 instant damage effects | 2.92 ms | 1.39 ms | – | 2.1× |
| spawn | 500 actors despawned, 500 spawned | 5.03 ms | 1.15 ms | – | 4.4× |

`–`: not run on the mt build. Both targets are met: `steady` single-threaded, `storm` with
`multi_threaded`.

## Scaling (st, ms)

| scenario | v1 1k | v1 5k | v1 10k | DAG 1k | DAG 5k | DAG 10k |
|---|---|---|---|---|---|---|
| idle | 0.036 | 0.133 | 0.252 | 0.032 | 0.123 | 0.235 |
| steady | 1.85 | 9.19 | 18.0 | 0.119 | 0.564 | 1.14 |
| storm | 9.60 | 51.5 | 95.7 | 0.838 | 4.35 | 9.63 |
| no_op_write | 8.33 | 41.0 | 82.0 | 0.074 | 0.363 | 0.859 |
| damage | 0.125 | 0.582 | 1.13 | 0.041 | 0.169 | 0.327 |
| aura | 1.41 | 1.46 | 1.60 | 0.460 | 0.547 | 0.668 |
| buff_churn | 1.98 | 10.2 | 22.2 | 0.225 | 1.21 | 3.16 |
| combat | 0.285 | 1.43 | 2.92 | 0.138 | 0.675 | 1.39 |
| spawn | 0.519 | 2.51 | 5.03 | 0.103 | 0.514 | 1.15 |

Both resolvers are linear in actors. The DAG resolver costs about 48 ns per resolved (actor,
attribute) pair single-threaded (`storm`: 9.63 ms / 200 000 pairs), against ~480 ns for v1.

## Reading the numbers

- **v1 does not scale to thousands of actors.** Each dirty attribute went through its own system,
  a recursive walk of effect entities, one observer trigger and one deferred `Dirty<T>`
  insert/remove per node. A rewrite that changed nothing still paid almost all of it
  (`no_op_write` ≈ `storm`).
- **The DAG resolver** keeps a folded `ModifierTable` per actor, so a base write recomputes one
  attribute from a cached fold instead of re-walking effects. Its first version measured 4.29 ms
  `steady` / 30.5 ms `storm`; reading components through pre-resolved `ComponentId`s instead of
  typed lookups and an inline 128-bit attribute set brought it to the table above.
- **Parallelism helps only when there is a lot to resolve.** `storm` gains 1.6× on the mt build,
  `steady` loses 50 % and `idle` 4×, because the multi-threaded executor's per-system dispatch
  dominates small ticks. The remaining floor is the 3 `FixedUpdate` nodes per attribute
  (periodic, instant base writes, base-write scan) — collapsing them into erased systems is the
  next optimization.

## P-12: expression shape (`leaf/expr_shape`, st)

| nodes | `Arc<dyn Expr>` | monomorphic | `HashMap<TypeId, f64>` |
|---|---|---|---|
| 1 | 6.66 ns | 6.14 ns | 7.11 ns |
| 3 | 13.5 ns | 11.1 ns | 13.8 ns |
| 7 | 26.4 ns | 23.1 ns | 28.0 ns |

The vtable costs 8–17 %, at or barely above the ~10 % floor (D-36), and the typemap is slower
than component reads at every size. Neither justifies a redesign. This run's loadavg was 4–20.
