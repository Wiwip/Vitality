# Verdict 2026-11-11 cycle 3 — sprint/2026-11-11-3 (R-9a, R-9b)

Range `32bbeea..26a7dea`, two commits, 16 files, +145/−140. Judged by the loop controller from a fresh
review per commit (`38eeb0cf` for R-9a, `78fe959b` for R-9b). Same model on both sides, so this carries a
`Pi-judged:` trailer and **no** `Verified-by:` (D-51).

## Per commit

- `bae6caa` **R-9a — PASS.** `cargo clippy -p vitality_core --all-targets` 17 → 3. Fixes: three private type
  aliases (`AttributeNode`, `PendingDrain`, `BaseScanQuery<T>`), `C: ContextMut` moved out of three angle-bracket
  bound lists into the existing `where` clause, `impl Clone for AttributeVar` reduced to `{ *self }`,
  `let_and_return`, `clone_on_copy`, and one `needless_range_loop` in the DAG topological sort.
- `26a7dea` **R-9b — PASS.** `cargo clippy -p vitality_effects --all-targets` 26 → 6 (the worker reported 27; the
  reviewer recounted at the base commit — 26 warnings, 20 fixed). The item's named priority, the `PeriodicState`
  7-tuple that this run's own R-3 landed (`9bd4bbd`), is now a module-private `PeriodicEffectRow` used at both of
  its former sites. Also five more aliases, the `PeriodicEffectRow`-style fixes for `condition.rs` /
  `effects_param.rs` / `resolver.rs` / `systems.rs`, and eleven single-line lint fixes. One refactor is not
  lint-shaped: the stacking guard pulled out of `spawn_persistent_effect` into `queue_stacked_effect` (8 args → 5).

## Verified on the range tip

`scripts/gate.sh` exit 0 at **105 passed / 0 failed / 0 ignored** (unchanged across both items, as it must be for
lint-only work). `test_effect_lifecycle` 8 passed under `VITALITY_RESOLVER=v1` **and** `=dag`; `test_stacking`
5, `test_bench_scenario` 7. `rustfmt --edition 2024 --check` clean on all 16 changed files, no `#[rustfmt::skip]`
in the reviewed ones. No suppression of any kind: no `#[allow]`, `#[expect]`, `cfg_attr(… allow)`, module-head
`#![allow]`, `clippy.toml`, or `--cap-lints`, and no file moved out of a lint's view. No `pub` signature, bound
set, `impl` header or `pub type` changed in either crate; all eight new aliases are private and expand to the
type they replaced, checked by mechanical expansion against `git show <base>:<path>`, not by eye.

The two fixes with real risk were checked structurally. `Clone` → `*self` is safe because the `Copy` impl at
`attributes.rs:482` carries exactly `Clone`'s bounds and `PhantomData<T>: Default` is unconditional, so the old
body built the same zero-sized field and a `Copy` type cannot have drop glue. The Kahn's-algorithm loop at
`registry.rs:312` scans the same indices in the same ascending order because `indegree` is `vec![0usize; n]` and
is only ever touched by `+= 1` / `-= 1`, so its length is still `n`; the DAG-order test ran in the gate. For R-9b
the review established the new control flow of the stacking refactor step by step (guard still after the `Instant`
branch, still before `attach_conditions`, same return values on the early paths) rather than accepting that the
tests are green.

## Nine warnings left, and why that is the honest floor

Core 3: `type_complexity` on the `AssignmentStep` returned by the public `Attribute::add`/`sub`/`set` — two of the
generic args are return-position-impl-trait plus an unnameable closure, so no alias can name the type (TAIT is
unstable) and the worker measured eight spellings that all still lint. **D-57** carries the one shape that does
clear it (a sealed `Setter` bound trait) for your decision; it is a public-API change, so it was not taken.

Effects 6: two 12-argument systems (`systems.rs:51` is `pub`; `:98` is private but grouping its params needs a
context struct whose lifetime story the review could not confirm), a Bevy observer at 8 arguments, and three
`only_used_in_recursion` params that are dead but whose removal would delete two `Res<>` presence requirements and
a query access from the pub v1 system — a change to what panics at runtime, which is not a lint item's to make.

## Where the reviews added value beyond approval

`78fe959b` **falsified two of the worker's justifications**: `Query`'s data-type param is covariant, so the
invariance argument for `systems.rs:98` does not hold (the conclusion may still be right, it is simply unproven),
and `only_used_in_recursion` is silenceable by the `_`-rename the lint itself suggests — so 6 is the floor for
fixes, not for name-based silences. `38eeb0cf` proved the core claim instead of assuming it, by inserting a
name-only alias in a scratch copy and watching clippy still report 3. Both are cases of the reviewer testing the
*argument*, not the build.

## Queued out of this cycle, none started

R-9a1 (a NaN modifier silently becomes `max_value()` in `from_op_stacked` — pin HEAD first, behaviour is your
call), R-9a2 (`at`/`at_base` are one body twice), R-9a3 (`AttrId(i as u16)` unasserted against the 128 cap),
R-9b1 (**no test builds an `Instant` effect with a stacking policy**, so if `queue_stacked_effect` ever drifts
above the `Instant` branch it silently stops applying modifiers), R-8d (a batch of bench-support citations into
`systems.rs` that were already wrong at this cycle's base, reviewer-verified), R-11 (the dependency-cycle branch
has no test, which is also what makes D-48 unsafe to act on), and R-9c/R-9d/R-9e for the three crates still
carrying warnings (abilities 11, debug 8, vitality 8 plus its tests and benches).

## Measurement note

No benchmark was run this cycle and no timing claim is made. `cargo bench` and even `--no-run` were excluded by
brief (17-minute link on a box at load 9–30); `cargo check -p vitality --benches` was used instead to prove the
bench tree still compiles.

Pi-judged: sprint-reviewer 38eeb0cf (R-9a), 78fe959b (R-9b)
