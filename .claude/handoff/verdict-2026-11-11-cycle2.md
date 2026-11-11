# Verdict 2026-11-11 cycle 2 — sprint/2026-11-11-2 (R-3b, R-3c, R-7, R-8, R-8c)

Range `2427565..5e43798`, five commits, 13 files, +889/-96. Base branch `feat/api-and-resolver`,
loop branch `auto/2026-11-11`. Judged by the loop controller from a fresh range review
(run `102fd3be`, `sprint-reviewer`) plus the item-level reviews named below. Same model on both
sides, so this carries a `Pi-judged:` trailer and **no** `Verified-by:` (D-51).

## Per commit

- `543faf5` **R-3b — PASS.** The registration pin is now two counters over every schedule (systems, and
  world-wide `Observer` components) with the per-attribute delta asserted `== 0` as `isize` and the fixed
  part against `DAG_SYSTEMS_BASELINE = 46` / `DAG_OBSERVERS_BASELINE = 12`, each with its own message.
  Reproduced on the range tip: `B-1 dag a=2: DagCounters { systems: 46, observers: 12 }`, identical at
  a=22. The slope-only pin it replaced caught 1 of 9 injected per-attribute systems; the new one caught
  9 of 9, and the reviewer rebuilt the deleted assertion verbatim and watched it pass on all four
  injections it survived.
- `e5aa1ac` **R-3c — PASS.** Pins the behaviour literal (`a_fire_due_on_the_expiry_tick_beats_the_expiry`,
  expiry tick 32) and the declared `.before(tick_effect_durations)` edge through
  `Schedule::graph().dependency()`, gated to DAG. The item asked for an assertion that makes the edge
  load-bearing, shown red by deleting it; reversing the edge does exactly that — and it loses the fire in
  **both** resolvers (1.0 vs 2.0), so v1's correctness on the same order rests on an undeclared tie-break
  (`vitality_effects/src/lib.rs:158-160` registers a plain non-exclusive applier with no edge). That is
  D-53, parked with D-47.
- `a413d28` **R-7 — PASS after two BLOCKs and five fix passes.** `Workload::ALL` 9 → 14; applies now go
  through `commands().trigger` so the apply lands in the timed `tick()`; a guard test counts
  `ApplyEffectEvent` dispatches (0 after `drive()`, the id's share after `tick()`); and a sweep of bench
  prose that R-1..R-3 had made false. Both BLOCKs were earned: the first because `world.trigger()`
  dispatches **synchronously** (`bevy_ecs-0.20.1/src/observer/mod.rs:26,63-69,104`), so the flush move
  alone left the tag test, condition eval, stacking decision and snapshot capture in the untimed input
  phase; the second because a measured figure ("residue 3 of 20 at a=20") survived in two files eight
  lines above the print that contradicts it. All five new workloads are honest — the reviewer broke four
  of them with its own perturbations and a guard went red each time.
- `d84d6b7` **R-8 — PASS after two BLOCKs.** The plugin-author contract is now the crate-level doc of
  `vitality` with three doc examples that the gate executes (one `should_panic`, proved non-vacuous by
  two mutations), plus a README section and a corrected comment in `vitality_effects`. Its load-bearing
  sentence went through three versions: my item text's trigger, the worker's correction, and finally the
  order the reviewer measured — `EffectsPlugin::build` under `PerAttribute` with the attributes registered
  under `Dag` is the silent one; the reverse runs as a mixed-mode app that nothing pins. `ResolverMode`
  has three read sites, all build-time, which is why inserting a mode after build is a no-op.
- `5e43798` **R-8c — PASS.** Comment-only repair of the citations `d84d6b7`'s 154-line insertion shifted:
  `support/floor.rs:10` was pointing at a `use` block instead of `apply_clamps` (`vitality/src/lib.rs:291-295`),
  three citations into `vitality_effects/src/lib.rs:142-152` had lost their third `.register::<T>()`
  (now `:144-153`), and `test_ability_machine.rs:412`'s bare `src/lib.rs:102` is now the full crate-relative
  path to the duplicate-add guard at `:200-202`.

## Verified on the range tip

`scripts/gate.sh` exit 0, 105 passed / 0 failed / 0 ignored (34 test binaries + 5 doc targets;
`test_bench_scenario` 3 → 7, `vitality`'s doctests 0 passed + 1 ignored → 3 passed).
**v1 is byte-identical**: zero non-comment changed lines under `crates/*/src/**` across the whole range,
the only src edit being a three-line doc comment at `vitality_effects/src/lib.rs:137-139`. Every target
the range touched is green in default, `VITALITY_RESOLVER=dag` and `=v1`; the four dag failures in
`test_bench_harness` / `test_bench_periodic` are pre-existing `Dirty<T>` marker asserts, proven not to be
from this range because the files that build those binaries carry no non-comment change in it. No
protected path appears in any commit; `AGENTS.md` is dirty in the tree and in no commit; nothing staged,
nothing untracked. No queued-but-unstarted item was half-done.

## Decisions this cycle depends on

**D-52** (does DAG mode require `EffectsPlugin`, or do the collapsed nodes move behind a first-install
guard?) and **D-53** (declare the periodic-before-expiry edge in v1 too?) are both owner calls, executed
as "document HEAD, change nothing". **D-56** is a potential library bug this run found and did not chase:
a live self-targeted modifier does not follow a base write in a dense 20-attribute world while the
dependency edge is recorded, and the same behaviour works in a plain app — both resolvers. It is the
most substantive thing here and it deserves its own hour, not the tail of a sprint.

## What the next cycle inherits

1. **The two harnesses default to opposite resolver modes** (`tests/gameplay/mod.rs:29-33` → dag,
   `benches/support/common.rs:37-39` → v1) and the gate never runs the bench-harness binaries under dag,
   which is how four dag failures survived every gate. Queued as R-10.
2. R-7's holes: `tick()`'s flush has no teeth (deleting it is mutation-green in both modes), `Spawn`'s
   two halves are unpinned (re-adding the flush there left all 18 tests green; `world.despawn` runs
   untimed), the periodic driver swap is unobservable, and the new guards pin only `ACTORS = 40` — the
   10k shape behind `steady < 2 ms` / `storm < 8 ms` is unpinned. Queued as R-7b/c/d/e.
3. R-8's mixed-mode app is now a doc claim with no test behind it; any change to `init_effect_attribute`'s
   early return makes the doc false silently.
4. `test_registration_cost.rs`'s header numbers still contradict the print in the same file (queued in
   R-9e), and `STACK_CAP = u32::MAX` means the clamped-stack path is priced by nothing.
5. Doc-comment line pointers are still maintained by hand. This cycle broke one purely by inserting a
   doc comment, with nobody touching a number — see D-55, which now has two independent incidents behind it.

## Measurement note

No number from this session's measurements should be quoted as absolute. The host sat at load 9–33 on
8 cores for most of the run, and an interleaved control on identical code swung 7.6 % peak-to-peak at
load 22–25. `.auto/BENCH-RESULTS.md` records the 10k rows with that caveat and withdraws R-7's
first A/B table outright, whose percentages did not follow from the numbers printed beside them.

Pi-judged: sprint-reviewer 102fd3be (range), 54e0abdb, 489d6964, 2f2fa2fb (R-7), 1adaf640, 3f57f72f (R-8)
