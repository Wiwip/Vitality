# SPRINT-SUMMARY — sprint/2026-11-07 (cycle 5, base `auto/2026-11-07`)

Stopped because the queue emptied: both scout items landed, one of them after this run's first review BLOCK. Cycle 5 also found the thing four judged sprints could not see, because it sat outside the check this loop chose to run.

| item | commit | verdict | one line |
| --- | --- | --- | --- |
| T-8 make the two `EffectBuilder` doc examples compile | `c102e0e` | OK | `cargo test --offline` is green end to end for the first time in this run |
| T-9 pin the cooldown's length, not just that it ends | `9b19ab2` | OK after BLOCK + one fix pass | the seconds/millis unit error now fails a test, at ±1 fixed step |

Gate at the tip: `bash scripts/gate.sh` exits 0 with 11 tests and 0 warnings; `cargo fmt --all -- --check`, `cargo check --offline --all-targets`, `cargo clippy --offline --all-targets` all exit 0, and `cargo test --offline --doc` gives 2 passed.

## Reviewer notes the owner should read

- **Your gate does not run doctests, and two of them have been broken this whole time.** `scripts/gate.sh` ends at `cargo test --offline --lib --tests`; `cargo check --all-targets` and `cargo clippy --all-targets` also exclude doctests, while `AGENTS.md` states the build loop as plain `cargo test --offline`. Both `EffectBuilder` examples failed to compile at every commit this loop judged. Adding `--doc` to the gate is a one-line change to a file this loop would not edit. Two caveats for running doctests here: they only execute when `LD_LIBRARY_PATH=$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/lib` is set, because `Cargo.toml:37` pulls bevy with `dynamic_linking`, and without it the runner dies on a missing `libstd-…so` while still printing the compile errors, which reads like a pass.
- **The cooldown is now pinned to ±1 fixed step, and the previous suite was looser than it looked.** `Duration::from_millis` instead of `Duration::from_secs_f32` — the classic unit error, truncating `0.25` to a zero-length timer — passed all 11 tests before this sprint. Now: passes for timers in (0.234375s, 0.265625s], so anything off by more than one 15.625ms step fails. Anything off by less than a step still passes, and nothing pins the exact expiry tick; that is inherent to driving a discrete fixed clock.
- **The first BLOCK of this run was a coverage regression in an item that only added lines.** Advancing "half the cooldown" spends ticks from the same `StateTimer`, so the existing "cooldown expires" assert went from catching any timer over 17 steps to tolerating 25. A cooldown 7% too long was red at base and green with that version. Measured both ways, fixed by driving every step in one loop. Worth carrying into your own reviews: an added assertion that shares a clock with an existing one changes what the existing one tests.
- **`run_out_cooldown` is gone from `tests/test_ability_machine.rs`.** It had one call site and a helper cannot host a mid-loop assert. Nothing else lost coverage, but the file no longer has a reusable cooldown driver, so a second cooldown test re-derives the timestep, the step count and the `ManualDuration` strategy inline.
- **Doc prose can regenerate the defect T-8 just fixed.** `src/effect/builder.rs:59-60` still points at `crate::attributes::Lit`, `::AttributeValue` and `::ValueSource`, which no longer exist since values moved to express-it; that stale vocabulary is what produced `Damage::value()` in a compiled example. Doctests catch a non-compiling example, never a wrong sentence. Separately, the second example is prose-called "damage over time" but keeps `ModOp::Add` to the target's Health, pre-existing and outside "make it compile".

## Edge cases

- T-8: neither fence asserts anything, so the doctests pin compilation only — a flipped `ModOp`, a wrong `EffectSubject`, or modifiers pushed in the wrong order stay green.
- T-9: `in_state(..).unwrap()` still reports a lost `MachineInstance<AbilityMachine>` as a parameter-validation panic rather than a failed assertion, file-wide convention untouched.
- T-9: the assert message no longer contains the string quoted in `SPRINT-TODO.md` T-9, superseded by the fix; a checklist grepping for it will not match.
- all: still true from cycle 1 — this branch does not build on its own, because the bevy `0.20` bump and `hfsm_bevy = { path = "../hfsm-rs/crates/hfsm_bevy" }` live in your uncommitted `Cargo.toml`, which `land.sh` never stages. `main` cannot build at all, since its manifest still points at `path = "../hfsm_bevy"`, a directory that does not exist.

## Try it

1. See the blind spot and close it: `cargo test --offline --doc` (with the `LD_LIBRARY_PATH` above) is 2 passed now; at `deffa8b` it was 2 compile failures. Then add `--doc` to your gate and watch how long it stays green.
2. Break the cooldown on purpose: at `src/ability/ability_state.rs:255` write `Duration::from_millis(new_cooldown as u64)` and run `cargo test --test test_ability_machine` — the lower assert fails. Then try `new_cooldown * 1.07`, which the previous suite also caught and my first attempt at this item let through; the upper assert fails.
3. Read what is actually pinned: a timer of 15.5 steps (0.242s) passes. That is the real precision of this pin, not "0.25s exactly".

## Retro

- Costliest: T-9, at two worker runs, two reviews and a fifteen-row mutation table for a 22-line test change. The cost went into bounds measurement rather than into the edit, and that was the right trade — the first version was net-negative coverage and nothing but a mutation sweep could show it. Cheap by contrast: T-8, three lines, one run, and it repaired a check the project documents.
- Blocked: nothing, and the mirror's mtime hazard now bites every child in one direction or the other. This cycle it produced a false BLOCK-adjacent result during re-review: a gate run replayed a leftover mutated binary and reported a failure that did not exist in the tree. Every child here now verifies the run printed `Compiling vitality` before believing any result, green or red.
- Skipped or retried: one BLOCK, one fix pass, both recorded above. Two item texts needed correction at landing this cycle: T-9's half-advance recipe (harmful as written, replaced by the reviewer's one-loop shape) and T-8's implicit assumption that the gate would notice doctests, which it does not.
- The pattern worth acting on: four sprints of green gates coexisted with a red documented check. Any gate that is narrower than the project's own stated command needs to say so out loud, in the gate file, not only in this loop's decision log.
