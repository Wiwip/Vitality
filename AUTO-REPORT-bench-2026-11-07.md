# AUTO-REPORT — implement BENCH-PLAN.md

Loop ran 2026-11-07 17:27 → (still running) on `auto/2026-11-07`, sprint branches `sprint/2026-11-07`
(items P-1, P-2, P-3, P-5) and `sprint/2026-11-07-2` (P-6, P-13). Base branch `workspace-split`. Every
commit came from `~/.pi/agent/sprint/land.sh` after a reviewer verdict; nothing was pushed and nothing
merged — the owner reviews and merges.

**Five items landed, all bench/test-only** (`crates/vitality/benches/**`, `crates/vitality/tests/**`):

| item | commit | what it established |
|---|---|---|
| P-1 harness | `6c80f80` | a world spec that really drives the resolver; every number below comes from it |
| P-2 leaf access paths | `8a010f6` | slot-block read 1.4× faster than the exclusion-bundle read, not the hoped 10–20× |
| P-3 exclusion-bundle scaling | `5ba12fa` | the bundle scan is **87 % of a warm leaf read** (5.79 ns → 43.0 ns at width 9) |
| P-5 idle floor | `458faac` | **4.0 µs per attribute per tick** with nothing dirty; 6 `FixedUpdate` nodes per attribute |
| P-6 recompute ratio | `12a52a1` | floor-subtracted ratio **≈ 1.0 at A=1/5/20** — each attribute pays for its own walk |

Gate (`bash scripts/gate.sh`, mirrored build) green at every landing; `cargo bench --offline --no-run`
green at every landing; `crates/vitality/tests/test_bench_harness.rs` now holds 5 tests, each of which
was observed failing before the code it guards existed.

---

## 1. The answer to the question the run was chartered on

*Is the per-attribute system approach sound, and if not, where does the cost actually go?*

**Per-attribute systems are the dominant cost, and it is not the arithmetic.**

- On a world of 1000 actors with **no effects and nothing dirty**, one `FixedUpdate` tick costs
  20.6 µs at 1 attribute, 82.3 µs at 20, 336.2 µs at 80. Least squares: **4032.9 ns per attribute per
  tick**, intercept 7436 ns, R² 0.9934 (reviewer reproduced: 4092.0, R² 0.9928). Labels: `mt`/`dyn`,
  bench profile, loadavg 2.5–5.
- The mechanism is not iteration. `init_attribute::<T>` installs **six `FixedUpdate` system nodes per
  attribute** (`Schedule::systems_len()`: a=1 → 16, a=2 → 22, a=5 → 40, a=80 → 490). With nothing dirty
  all six fetch their parameters, find nothing, and return: **~670 ns per no-op node**, 486 of them at
  80 attributes. BENCH-PLAN.md:146's "≥ 2 µs per attribute per tick ⇒ the topology is the problem" row
  trips at **2.0×**.
- The real work is cheap by comparison. A warm leaf read through `EntityRefExcept::get::<T>()` is
  **43 ns**, of which **38 ns is bevy scanning the exclusion bundle** (one `Components::get_id(TypeId)`
  per excluded member, no early exit) and 5.8 ns is the read itself. Floor-subtracted, dirtying one
  attribute on one actor costs ~3.4 µs over the floor and dirtying 20 costs ~28 µs at 5 effects,
  ~130 µs at 100 effects.
- So the two costs are independent and both are worth fixing: **the schedule floor is per registered
  attribute**, and **the re-resolve work is per (attribute × effect-walk)**. Marginal ratio ≈ 1.0
  (1.00 / 0.98 / 1.07 for A=1/5/20, best-behaved at E=100: 1.00 / 0.77 / 1.07) says a shared pass cannot
  remove the per-attribute walk — but a single walk reused across attributes would remove the other
  A−1 of them, which at E=100 is ~130 µs → ~6.5 µs for one actor.

**On parallelizing by attribute:** the data argues against it. The per-attribute systems are already
independent and the `mt` executor is free to run them concurrently — the floor above was measured on an
`mt` build, and it is still 4 µs/attr/tick. The cost is the *number of graph nodes and parameter fetches*,
not a serialized chain, so more threads divide a cost that should be deleted instead. Parallelize the
actors inside one attribute-agnostic resolver; the actor dimension is where independence exists.

## 2. Two bugs the reviews turned up (both library-side, both out of this loop's scope)

1. **A dirty attribute never becomes clean again.** `update_attribute` writes through `&'static mut T`
   (`vitality_core/src/attributes.rs:315-319`), which sets `T`'s change tick;
   `on_change_notify_attribute_parents::<T>` (`vitality_effects/src/systems.rs:274-283`, in
   `EffectsSet::Notify`, after the resolve) matches `Query<Entity, Changed<T>>` and re-triggers
   `MarkNodeDirty<T>`, re-inserting `Dirty<T>`. A reviewer measured it at this tip: mark once, then
   `Dirty<T>` present and `Changed<T>` matching **every tick forever**, value unmoving. Nothing in the
   tree pins this. Consequence: the "idle floor" is not the steady state for anyone who changes a stat
   once — you re-resolve that attribute every tick for the life of the entity. This is the single
   highest-value fix in the list and it is a correctness issue before it is a performance one.
2. **`bundle_contains_component` has no early exit** and does a `TypeId` hash lookup per excluded member,
   so leaf reads scale with the exclusion list (23–31 ns per member cold, ~4.2 ns warm). The expression
   leaf path is the one that pays it.

Also found in the harness (bench-side, being fixed in P-13): `common.rs` documented `dirty_fraction` as
the share left dirty at end of build when the build leaves *everything* dirty, and
`mark_dirty_fraction`'s marks were deferred (a `World::trigger` whose observer insert sits in the
deferred queue without a `flush()`), so it returned a count it had not observably marked.

## 3. What the owner has to decide

- **Manifest, protected here**: the test/bench profile is `mt`/`dyn` because the dev-dependency bevy
  pulls `ui → default_platform → multi_threaded` and `dynamic_linking`. That means (a) BENCH-PLAN.md:146's
  row, keyed on `st`/`static`, cannot be satisfied in-repo today, and (b) the shipped labels said `st`
  until P-13 corrects them to `mt`/`dyn`. A `bench-threads`-style feature and dropping
  `dynamic_linking` from the dev dep are both one-line manifest changes I was not allowed to make.
- **`scripts/**` is protected**, so `scripts/bench.sh` (P-11) was skipped and the run commands live in the
  bench target's module doc instead. `scripts/gate.sh` had lost its executable bit (`644`, invisible to
  `git status` because `core.fileMode=false`), which made every `land.sh` exit 14; restored with
  `chmod +x` — untracked, so the owner should re-check it.
- **Gate cost**: the 80-attribute pool P-5 needed adds ~27 s to a full test-target build and ~8 min to a
  cold bench-target build. If that is too much for the gate, say so and the 80-point can be gated out.
- **Items the plan wanted that this loop did not reach**: P-4 (registration counts as a test), P-7
  (actor scaling and the shard boundary), P-8 (write churn / dirty storm — now more interesting given
  bug 1), P-9 (periodic path), P-10 (`st` vs `mt` probe without touching a manifest), P-12 (expression
  tree and typemap comparisons).

## 4. Limits on these numbers — read before quoting

- Everything is **`mt`/`dyn`** (see above). Micro-cost numbers (P-2/P-3) are not materially affected; the
  floor is, and the direction of the correction is unknown — do not extrapolate it to a static build.
- Absolute ns values track box load (loadavg 2.4–8.6 during the run); **ratios are the stable signal**.
- The raw P-6 ratio (`all/(A×one)`: 1.01 / 0.26 / 0.07) is **not** the result; at A=20 the denominator is
  95 % idle schedule. Only the floor-subtracted marginal means what §2B intended.
- At A=1 the "one attribute dirty" and "all dirty" sets coincide, so A=1 rows are tautologies; at E=1–5
  the walk term is at the noise floor and two points (−0.33, 1.65) came out of denominators inside their
  own confidence intervals. Decision-grade: E=20–100.
- Recompute shapes are **one actor**; floor shapes are 1000 actors with no effects. Neither is a game
  profile. `dirty_fraction` in `common.rs` is still not a working knob for shapes that need partial
  dirtiness (P-13 documents it; P-8 would exercise it).
- Harness guards compare per-actor values, and the generators can give every actor identical values, so a
  helper reading the wrong actor can still pass. A per-actor identity guard needs a `common.rs` change
  that was out of scope for the items that noticed it.

## 5. Suggested order of attack

1. Fix the re-mark loop so dirtiness is value-driven or queue-driven, not `Changed<T>`-driven. Deletes the
   forever-dirty steady state and lets one resolver walk only what changed.
2. Replace the six `FixedUpdate` nodes per attribute with one attribute-agnostic pass over a static
   attribute table (dependency edges are already discoverable from `get_dependencies()`). Worth ~4 µs per
   attribute per tick unconditionally; at 20 attributes that is the whole 82 µs floor.
3. Narrow the leaf exclusion bundle and pre-resolve `ComponentId`s at registration. Worth ~38 ns per leaf
   read — small in isolation, but it is paid once per expression leaf per walk, so it multiplies with the
   E=100 case above.
4. Only then revisit threads: enable `multi_threaded` deliberately in the library build and parallelize
   **actors**, not attributes; re-measure with the same ids so the delta is attributable.

## 6. Loop housekeeping

`.auto/TODO.md`, `.auto/LOG.md`, `.auto/decisions.md` and `.auto/state` (all git-ignored) carry the full
trail: every worker and reviewer run id, the two BLOCK verdicts and what they forced, the D-11…D-22
decisions, and the parked items. One process fix worth knowing: a claimed worker resume must be issued in
the same controller turn that claims it — the loop once sat idle on a resume it had only written about.
