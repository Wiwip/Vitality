#![allow(dead_code)]

//! Filter-safe post-loop guards (`.auto/TODO.md` B-5).
//!
//! A criterion filter decides whether a benchmark's *closure* runs, not the code around it:
//! criterion 0.8.2 `benchmark_group.rs:303-324` gates `analysis::common(...)` on
//! `filter_matches(id.id())`, and `criterion_group!` calls the group function either way. So an
//! `assert!` written after a `bencher.iter(...)` loop still executes on a filtered run, on state the
//! loop never touched. Two ways that goes wrong, and the second is the bad one:
//!
//! * **panic** — a `dirty_entity_count == 0` guard or an "every reader moved" guard on a world that
//!   was never ticked aborts a run that had already done all its work;
//! * **pass vacuously** — a guard whose condition a settled world happens to satisfy (`0 == 0`,
//!   marks a shape applied in setup rather than in the loop) prints nothing and reports green for a
//!   check that looked at nothing.
//!
//! [`Ran`] is the one idiom for both: the timed closure bumps it once per iteration, and the guard
//! after the loop runs only if the loop did, printing `guard skipped (<id> filtered out)` when it
//! does not. A guard that ran asserts exactly what it asserted before this existed, and prints
//! nothing extra.
//!
//! The groups whose asserts already sit *inside* their closure need no [`Ran`]:
//! `harness/app_update` keeps both of its asserts in the `iter_batched_ref` setup closure, and the
//! three `leaf/*` groups have no assert outside a closure at all. A filter that drops a sample drops
//! the assert that rides in it, so those groups can never look at a world they did not build — which
//! is the property everything else here has to re-create by hand.
//!
//! What it does **not** guarantee: that an assert outside a loop needs it — a guard that only reads
//! what the group function itself built (the pre-loop `attrs().len()` and `shape.ticking` asserts) is
//! true under every filter and stays bare; nor that a guard that ran saw a full measurement, only
//! that the closure ran at least once, and a short `--sample-size` run satisfies that just as a full
//! one does, so the guard cannot tell the two apart; nor anything about a loop written against a
//! differently named `Bencher` binding.
//!
//! The counter is a [`Cell`], not an `AtomicU64`: `criterion_main!` runs benchmarks one after
//! another on the main thread, so the bump is a plain load-and-store inside the timed loop rather
//! than a locked read-modify-write.
//!
//! Criterion-free so `tests/test_bench_guards.rs` can include it (rule 3 of
//! `tests/test_bench_wiring.rs`).

use std::cell::Cell;

/// How many iterations this benchmark's closure actually ran, read by the guard after its loop.
pub struct Ran {
    iterations: Cell<u64>,
}

impl Ran {
    pub fn new() -> Self {
        Self {
            iterations: Cell::new(0),
        }
    }

    /// Call once per timed iteration, from inside the closure.
    #[inline]
    pub fn bump(&self) {
        self.iterations.set(self.iterations.get() + 1);
    }

    pub fn iterations(&self) -> u64 {
        self.iterations.get()
    }

    pub fn ran(&self) -> bool {
        self.iterations.get() > 0
    }

    /// Assert what `check` asserts if the loop ran, and say so out loud when it did not. Returns
    /// whether the guard ran, so a caller can see the difference too.
    pub fn guard(&self, id: &str, check: impl FnOnce()) -> bool {
        if self.ran() {
            check();
            true
        } else {
            println!("{}", skip_line(id));
            false
        }
    }
}

/// The line a skipped guard prints, kept apart from [`Ran::guard`] so the wording a run is judged on
/// is testable without one.
pub fn skip_line(id: &str) -> String {
    format!("guard skipped ({id} filtered out)")
}
