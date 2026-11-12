# Verdict 2026-11-11 cycle 4 — sprint/2026-11-11-4 (R-9b1)

Range `535cd8d..be06328`, one commit, one file, +69/−0. Judged by the loop controller from review run
`1949db87` (`sprint-reviewer`), which reproduced the mutation itself. `Pi-judged:` trailer, **no**
`Verified-by:` (D-51).

## `be06328` **R-9b1 — PASS**

`instant_effect_with_a_stacking_policy_applies_but_never_stacks` in
`crates/vitality/tests/test_stacking.rs`, pinning that `queue_stacked_effect` runs **after** the `Instant`
branch in `apply_effect_event_observer` (`vitality_effects/src/application.rs:339-357`, guard at `:359-361`).
An addition-only diff: no `src/**` file differs from the base commit, verified with `git diff` rather than the
worker's `md5` claim.

Gate **106 passed / 0 failed / 0 ignored** (101 in `--lib --tests` across 34 binaries plus 5 doctests), up from
105 with nothing else moving. `test_stacking` is 5 → 6 and passes under `VITALITY_RESOLVER=v1` and `=dag`.

**Teeth, reproduced independently.** The reviewer backed `application.rs` up to `/tmp`, moved the guard above the
`Instant` branch, and got the new test red in **both** modes with
`assertion left == right failed: the instant application must still write its modifier / left: 50.0 right: 40.0`
(`test_stacking.rs:158`), then restored by byte copy and proved the tree clean again. No git state-changing command
was used by the reviewer.

## The finding that shaped the test

The obvious version of this pin — apply an `instant()` def carrying an `Add` stacking policy twice, assert no
stack — **cannot fail**, and the worker only knew that because it wrote it and ran it under the mutation: it
passed. `AppliedEffects` is filled only through the `EffectTarget` relationship (`components.rs:76-82`), inserted
at exactly one site (`application.rs:262`, inside `spawn_persistent_effect`); the `Instant` branch returns before
spawning, so the handle is never in `applied` and `queue_stacked_effect` returns false regardless of ordering.

The shipped test therefore establishes the precondition the guard actually needs — the target still holds an
applied instance of the same handle — by applying the def while it is `permanent` and then reloading that asset in
place with `Assets::get_mut`. The review confirmed this is a state a host reaches, not a back door: the observer
takes `Res<Assets<EffectDef>>` and looks the handle up at apply time (`application.rs:313-318`), `Effects::apply`
only carries the handle, and nothing in the workspace snapshots or compiles a def at `add` time (seven live-read
sites checked). Assertions are literals throughout: `base == 40.0`, `stacks == Some(1)`,
`EffectStacksChanged` recorded set `== Vec::new()`, `EffectApplied.len() == 2` with the per-entry `effect` field.

## Limits of the pin, stated by the review

It pins guard-vs-`Instant` only. Moving the guard above `trigger.cleanse(...)` stays green, so an effect that both
cleanses and stacks is unprotected; and the premise keys on the handle id surviving the reload, so a host that
reloads by producing a *new* `Handle` is outside its reach. Neither is a defect in the item — both are the shape of
what is left.

## Queued from this cycle

**R-9b2**: the two orderings this pin does not reach — guard-above-`cleanse` stays green, and a host that
reloads by producing a *new* `Handle` never matches the guard's handle-identity test (and if that is how hosts
really hot-reload, the identity match itself is the thing to revisit, which is a behaviour call).
Still open from earlier cycles: R-9a1 (NaN → `max_value()` in `from_op_stacked`), R-9a2, R-9a3,
R-8d (bench citation rot), R-11 (dependency-cycle branch untested, which is what makes D-48 unsafe), and
R-9c/R-9d/R-9e (abilities 11, debug 8, vitality 8 warnings). D-57 stays yours.

Pi-judged: sprint-reviewer 1949db87 (R-9b1)
