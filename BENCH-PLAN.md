# BENCH-PLAN — measuring the per-attribute stat resolver

Proposed at tip `d0e9e6d`. No bench data exists today: `crates/vitality/benches/bench_systems.rs` has
every body commented out and its `ActorBuilder::new(entity)` / `EffectBuilder::new(source, effect)`
calls no longer compile against `crates/vitality/src/actors.rs:104`. So this plan's first job is a
harness that can be trusted, and its second job is to be cheap enough to run in the mirror sandbox.

Three cost theories are currently unfalsifiable, and they imply three different refactors:

| theory | if true | refactor it justifies |
| --- | --- | --- |
| T1 the floor is the schedule: ~25 ECS items per attribute run every tick whether or not anything is dirty | `crates/vitality_core/src/lib.rs:120` + `crates/vitality_effects/src/lib.rs:72` per attribute | collapse to one system + an erased op table |
| T2 the recompute walk is multiplied by attribute count (`AppliedEffects` is untyped) | `vitality_effects/src/systems.rs` walks all effects per `T` | one attribute-agnostic gather pass |
| T3 the dominant cost is expression leaf access: `EntityRefExcept::get::<T>()` linear-scans the ~20-component exclusion bundle | `attributes.rs` `Attribute::at()` → `view.rs:19` | pre-resolve ids / slot block, before touching anything else |

Every bench below exists to put a number on one of these. A bench that doesn't discriminate between
them is out of scope.

## 0. Ownership and sprint scope

Much of this plan needs paths a sprint may not touch (`scripts/**`, `Cargo.toml`, `crates/*/Cargo.toml`).
Split it so nothing blocks mid-sprint:

- **Owner, before the sprint:** the `[[bench]]` entries (one per file in §4, `harness = false`), a
  `multi_threaded` feature on `crates/vitality` forwarding to bevy's, a `bench-alloc` feature, a way to
  build without `dynamic_linking` (move it behind a feature, or drop it from the dev-deps), and
  `scripts/bench.sh`. Until the `dynamic_linking` change lands, every duration is `dyn` and no `static`
  number exists.
- **Sprint (max 4 items, test first):** (1) a compiling harness: `common.rs` + `WorldSpec`, replacing
  `bench_systems.rs`; (2) `leaf.rs` (family C); (3) the G counter test; (4) `floor.rs`. Nothing else is
  in a sprint; families B, D, E, F are follow-ups once C and A have produced a decision.

## 1. Harness rules

These make numbers comparable. Violate them and the result is unusable.

1. **Deterministic clock.** `TimeUpdateStrategy::FixedTimesteps(1)` (`bevy_time-0.20.1/src/lib.rs:127-128,187-189`) — one `app.update()` is exactly one fixed tick. Report ns/tick. Never the wall clock.
2. **Isolate the schedule for phase benches.** `world.run_schedule(FixedUpdate)` (`bevy_ecs/src/world/mod.rs:4001`) so asset/UI/IO work isn't in the number. Keep exactly one whole-`app.update()` bench as the reality check.
3. **Setup out of the measured loop.** `iter_batched(.., BatchSize::LargeSample)` (`criterion-0.8.2/src/bencher.rs:234`, `lib.rs:191`). Two shapes, both needed: *steady-state* (build once, tick N times — the number the game feels) and *cold* (rebuild per sample, includes archetype/required-component work).
4. **Feature matrix, named baselines.** Two axes, never compared across:
   - `multi_threaded`: off today (`default-features = false` + `["bevy_log","bevy_asset","bevy_scene"]`; bevy 0.20's `default` is `["2d","3d","ui","audio"]` and only `default_platform` pulls `multi_threaded`). Without it the executor is single-threaded (`bevy_ecs/src/schedule/executor/mod.rs:46-60`) and `Query::par_iter*` compiles to a sequential fold (`query/par_iter.rs:87-102`). Baselines `st` / `mt`.
   - `dynamic_linking`: the dev-dependency at `crates/vitality/Cargo.toml:28-37` turns it on for every bench and test today, which means `-C prefer-dynamic` and no cross-crate inlining of bevy generics — it inflates per-system and per-query cost. Every number must be labelled `dyn` or `static`; a shipped game is `static`.
5. **Shape in the bench id.** `recompute/a=20/e=20/actors=1000/dirty=10/st/dyn`. No bare numbers may be quoted without their id.
6. **Seeded RNG.** `StdRng::seed_from_u64(7)` (rand 0.10) so every run builds an identical world.
7. **Baselines.** `cargo bench -- --save-baseline <branch>`; regressions checked with `--baseline main`, which criterion prints natively (`lib.rs:669`). No critcmp — no network here.
8. **Mirror only.** Benches run through a `scripts/bench.sh` (owner-written, a clone of `scripts/gate.sh`) (writable `/tmp` mirror, `CARGO_HOME=/tmp/ch`, `--offline`). Two hazards that already bit this repo, both inherited: add `touch` after the `tar` extraction (restored mtimes replay stale binaries), and treat a run as untrustworthy unless it printed `Compiling vitality`.

## 2. Bench families

### A — floor: what an attribute costs when nothing happens (T1)

| id | shape | metric | a bad number means |
| --- | --- | --- | --- |
| `floor/idle_tick` | N attributes registered, 1000 clean actors, zero dirty. N ∈ {1,5,10,20,40,80} | ns/tick vs N; fit the slope | slope is the per-attribute scheduling floor. At 40 attributes a slope >2 µs/attr/tick is >1% of a 60 Hz tick spent doing nothing |
| `floor/register_and_build` | `App::new()` + `AttributesPlugin` + N × `init_attribute` | ms to build, plus counters below | build-time and runtime-registration cost per attribute |
| `floor/spawn_actor` | 1000 spawns of an actor with N attributes | µs/spawn, `Throughput::Elements` | required-component (`AttributeCalculatorCached<T>`) and archetype-growth cost per attribute |

### B — recompute path: is the walk multiplied (T2)

All of these hold the *work* constant and vary how it is spread, because the discriminator is a ratio, not a level.

| id | shape | metric | a bad number means |
| --- | --- | --- | --- |
| `recompute/one_attr` | 1 actor, A attributes, E permanent effects × 1 modifier; dirtied: 1 attribute. E ∈ {1,5,20,100} × A ∈ {1,5,20} | µs/effect | if µs/effect grows with A, the walk is per-attribute — T2 confirmed |
| `recompute/all_attrs` | same world, all A attributes dirty in one tick | ratio: (cost(all) − idle(A)) / (A × (cost(one) − idle(A))), where idle(A) is `floor/idle_tick` at the same A, so T1's floor is not counted as walk cost | ≈1.0 → the walk is repeated A times (no sharing). ≪1.0 → shared, T2 refuted. **This single ratio is the refactor decision** |
| `recompute/many_actors` | {100, 1k, 10k} actors × {0,1,10,100}% dirty × A ∈ {5,20,40} | actors/s | cache behaviour across A separate storages per actor; where the per-actor shard boundary should be |
| `recompute/cross_actor` | E effects whose modifiers read *another* actor's attribute (`update_modifier_when_dependencies_changed` cascade) | µs/tick + passes-to-converge counter | cost of the event cascade and of the fixed-point iteration; passes > 1 means per-tick staleness |

### C — leaf access microbenches (T3) — do these first

Cheapest to write, and they change how every other number should be read.

| id | shape | metric |
| --- | --- | --- |
| `leaf/access_paths` | evaluate `Health::dst() + MaxHealth::dst()` four ways: (i) `EntityRefExcept::get::<T>()` with the real exclusion bundle, (ii) plain `EntityRef`, (iii) `get_by_id` with a pre-resolved `ComponentId`, (iv) a `Vec<f64>` slot component indexed by `u16` (the target design, as a reference line) | ns per leaf read |
| `leaf/exclusion_length` | same read, exclusion list length ∈ {0,5,20,40} | ns vs length — proves the linear scan (`bevy_ecs/src/world/entity_access/except.rs:545-554`, `bundle/impls.rs:24,81`) and prices "shrink the exclusion bundle" |
| `leaf/expr_depth` | 1, 3, 8-node expressions via `Arc<dyn Expr>` vs a monomorphic equivalent | ns/node — prices the vtable tax, which is already paid today |
| `leaf/typed_vs_hashmap` | the above against a `HashMap<TypeId, f64>` typemap, the usual alternative | ns/access — kills or validates the typemap option with data |

Why C is first: `Components::get_id` is a `TypeId` hash lookup with an `RwLock::read()` fallback for
unregistered types (`component/info.rs:625-631`), and `bundle_contains_component` resolves **every**
excluded type through that map on **every** `get` (`except.rs:92-99`). A leaf read is plausibly
~20+ hash lookups. If that's true it dominates both T1 and T2 and re-ranks the whole plan.

### D — write churn (change detection and the notify path)

| id | shape | metric | a bad number means |
| --- | --- | --- | --- |
| `churn/apply_instant` | N instant damage effects per tick (message → base write → `MarkNodeDirty` → walk) | effects/s | the cost of the damage path, the thing a real game spams |
| `churn/dirty_storm` | every actor dirty every tick | actors/s | worst case: many auras in a boss fight. This is the number to design the ceiling against |
| `churn/no_op_write` | write the same value every tick vs write only when `are_different` says it changed | ratio | prices the missing value guard. Assumes `Changed<T>` fires on any `DerefMut` write and that a fire triggers a full walk; confirm both in code before building this bench |
| `churn/clamp_chain` | `Health` clamped to `MaxHealth`, `MaxHealth` written every tick | µs/tick | clamp path cost, and the fan-out cost once `clamp_reverse_lookup` stops being a stub (`vitality/src/clamp.rs`, `crates/vitality/src/actors.rs:140`) |
| `churn/stacks` | stack add/remove on a stacking effect | µs/operation | rebuff cost |

### E — periodic/ticking path

| id | shape | metric | a bad number means |
| --- | --- | --- | --- |
| `periodic/tick_scan` | T ticking effects, ~1 firing per tick, A ∈ {1,5,20} | ns/tick vs A | `apply_periodic_effect::<T>` scans all ticking effects per attribute (`vitality_effects/src/lib.rs:79`), so growth in A here is the same multiplication as T2 on a second code path |
| `periodic/all_fire` | every ticking effect fires in one tick | effects/s | the burst case: DoT refresh on a whole raid |

### F — the parallelism decision (write the prototype *with* these)

| id | shape | metric |
| --- | --- | --- |
| `parallel/feature_probe` | `recompute/many_actors` and `churn/dirty_storm` re-run with `multi_threaded` on vs off | speedup of the *current* design. If ~1.0, the per-attribute design is currently paying its cost for a benefit that is compiled out |
| `parallel/shard_axis` | current per-attribute systems vs a throwaway single resolver sharded by actor (`Query::par_iter_many_unique_mut`, `system/query.rs:1460`) on the same workloads | µs/tick at threads ∈ {1,2,4,8}. Report the single-threaded column honestly: a design that loses on 1 thread is a bad design for wasm |
| `parallel/api_floor` | `par_iter` vs `iter` over 10k entities, sweeping `BatchingStrategy` | fixed cost per parallel dispatch (µs of thread wakeup). Anything below that must not be sharded |

### G — counters, not durations (cheap, and belongs in the gate)

A `#[test]`, not a bench, so it runs on every landing:

```rust
// crates/vitality/tests/test_registration_cost.rs
let s = |n: usize| app_with_n_attributes(n)
    .world().resource::<Schedules>().get(FixedUpdate).unwrap().systems_len();
let per_attr = (s(22) - s(2)) / 20;                       // schedule/schedule.rs:745
let comps    = world.storages().components().len();        // component/info.rs:420
let reflect  = app.world().resource::<AppTypeRegistry>().0.read().len(); // type_registry.rs:790
assert!(per_attr <= BASELINE, "{per_attr} systems per attribute");
```

`BASELINE` is the measured systems-per-attribute at the tip when the test lands, not a guess. T1's "~25"
counts observers and messages too, which `systems_len()` doesn't, so the two numbers differ by design.

Plus, per run, a committed `BENCH-RESULTS.md` row for: component bytes per actor per 10 attributes;
`cargo check --offline` seconds with 10 vs 40 `attribute!` declarations (monomorphization + reflection
codegen — a script, since criterion can't see compile time); and allocations per tick from a
hand-written counting `#[global allocator]` behind `#[cfg(feature = "bench-alloc")]` (no new
dependency, which the sprint rules forbid anyway). Allocation counting is aimed at two known sites:
a `Vec` per `neighbors()` call (`vitality_effects/src/graph.rs:91`) and a `HashSet` + `Vec` per
notification (`vitality_effects/src/systems.rs:262`).

## 3. Decision thresholds

Write these down before running, so the numbers can't be argued backwards:

| observation | decision |
| --- | --- |
| `leaf/access_paths`: (i) ≥ 3× (iii) | fix leaf access first — pre-resolved ids and a narrower exclusion bundle. Re-run everything after; it changes the ranking of every other bench |
| `recompute/all_attrs` ratio ≥ 0.7 | walk is multiplied. One attribute-agnostic gather pass is required; keep or drop per-attribute systems after |
| `recompute/all_attrs` ratio ≤ 0.3 | walk is effectively shared. Keep the design, do the hardening list only |
| `recompute/all_attrs` ratio in (0.3, 0.7) | ambiguous: no refactor on this evidence. Re-run at larger A and E; decide on `floor/idle_tick` and `leaf` alone |
| `leaf/access_paths`: (i)/(iii) in (1×, 3×) | ambiguous: do the cheap exclusion-bundle shrink only, re-run, then re-read the ratio |
| `floor/idle_tick` slope ≥ 2 µs/attr/tick (`st`, `static`) | collapse the systems behind an erased op table regardless of the walk result |
| `parallel/feature_probe` speedup ≈ 1.0 | enable `multi_threaded` (or state in the README that vitality is single-threaded by design) before any refactor — the current design's one advantage is compiled off |
| `parallel/shard_axis` at 1 thread: per-actor ≥ 1.5× faster | build the single-resolver design |
| allocations/tick > 2× entities touched (entities whose `T` changed that tick) | fix the `Vec`/`HashSet` sites first; they're cheap and they pollute every duration bench |

## 4. Files

```
crates/vitality/benches/
  common.rs              # world builders + tick loop + counters + feature-labelled ids
  floor.rs               # A
  recompute.rs           # B
  leaf.rs                # C
  churn.rs               # D
  periodic.rs            # E
  parallel_probe.rs      # F  (required-features = ["multi_threaded"])
crates/vitality/tests/test_registration_cost.rs   # G
scripts/bench.sh         # mirror + touch + CARGO_HOME, arg = criterion filter
```

`[[bench]] harness = false` per file; `bench_systems.rs` is replaced, not extended. The shared
harness takes a `WorldSpec { attributes, effects_per_actor, modifier_shape, dirty_fraction,
ticking_fraction, cross_actor }` so every family draws from the same generator and the shape lands in
the bench id automatically.

The gate gets the G test only (it runs under `--tests` with no `gate.sh` change). Durations stay out
of the gate: timings here are directional (§7), so a regression threshold would flake. Duration
baselines are saved and compared by hand through `scripts/bench.sh`, against the sprint base branch
(`auto/2026-11-07`, not `main`), with the criterion baseline kept in the mirror's `target/`.

## 5. Order

1. Harness, then C (`leaf`, `exclusion_length`) — decides whether the rest matters.
2. `floor/idle_tick` + the G counters — prices the per-attribute registration directly.
3. `recompute/all_attrs` + `periodic/tick_scan` — the multiplication ratio, on both paths.
4. `parallel/feature_probe` — the current design's actual concurrency, with and without the feature.
5. F with a throwaway prototype resolver in a scratch branch — `parallel/shard_axis`.
6. Everything else as regression guard-rails once 1-5 has produced a decision.

## 6. Out of scope

`vitality_debug`/overlay (feature-gated, not in the hot path), the ability HSM machine and cooldowns
(separate axis, and already pinned by tests), global effects, asset loading. Reflection throughput is
only in scope through the G counters.

## 7. Known limits of this plan

- Duration benches on a single machine in this sandbox are directional: shared CPU, the profile in use (state whether the mirror
  builds benches with the `bench` profile or a dev-derived one), and the `dyn`/`static` split above. Quote confidence intervals,
  never point estimates, and never a `dyn` number as if it were a shipped-game number.
- Steady-state benches need the world *reset* between samples or the dirty-set drifts; the harness
  must re-mark explicitly rather than rely on the previous sample's state.
- The prototype in F5 is deliberately throwaway: it exists to produce a number, and it will ignore
  clamps, stacks, periodic effects and reflection. Its only claim is the shard-axis comparison.
- Thresholds in §3 are my guesses about what matters, set before measurement on purpose. If one looks
  wrong after C, change it in a commit that says why — not inside a results commit.
