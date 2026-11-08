#![allow(dead_code)]

//! P-14 (BENCH-PLAN §2G): a counting `#[global_allocator]` the gate can see.
//!
//! Criterion measures duration, and §3's last unmeasured row — "allocations/tick > 2× entities
//! touched" — is not a duration question. [`CountingAlloc`] wraps `std::alloc::System` with a pair of
//! relaxed `AtomicU64`s, [`counting`] decides whether they run, and [`counted`] returns what passed
//! through them. It stays criterion-free so `tests/test_bench_allocations.rs` can `#[path]` it, which
//! is the only way the instrument has any test at all: the gate never builds a bench target.
//!
//! Two known sites are what it is aimed at, both still in the tree:
//!
//! * `vitality_effects/src/graph.rs:91` — `effects.iter().collect::<Vec<_>>()` inside
//!   `IntoNeighbors::neighbors`, i.e. one `Vec` per graph node a resolve visits;
//! * `vitality_effects/src/systems.rs:259-268` — a `HashSet<Entity>` and a `Vec<Entity>` built per
//!   entity the `Changed<T>` notification walks.
//!
//! ## The gate is runtime, not a cargo feature, and that is what it costs
//! §2G asks for `#[cfg(feature = "bench-alloc")]`. `crates/vitality/Cargo.toml` is protected in this
//! sprint — no feature, no dependency — so the switch is a `static AtomicBool` that [`counting`]
//! flips. **With the gate off, every `alloc`, `alloc_zeroed` and `realloc` call pays one relaxed load
//! of that flag and one predictable branch, and `dealloc` pays nothing** (it does not read the flag).
//! That is the whole deviation from a cfg'd-out allocator, and it is not free for the neighbours:
//! `benches/bench_systems.rs` installs this wrapper, so every duration number that target produces
//! carries that load per allocation. Both sides of every §3 ratio, slope and pair pay it, so no
//! decision flips on it — but do not print a pre-P-14 ns/tick beside a post-P-14 one.
//!
//! ## Where the `#[global_allocator]` is declared, and what reaches it
//! Only `benches/bench_systems.rs` declares one, binding [`ALLOC`]. The test binary deliberately does
//! not: a wrapper there counts the whole process, other threads included, and that is what makes the
//! probe unpinable in it — with one installed, a stray `{allocs: 1, bytes: 48}` from a bevy pool thread
//! tripped the blindness assert in 2 of 3 gate-shaped runs.
//!
//! Open gap: the probe's composition is checked by the [`calibration_line`] each bench run prints, not by
//! a gate test; the machine-checkable version needs a bench-profile-only instrument (protected manifest,
//! parked).
//!
//! What reaches the wrapper is only the allocation calls a binary **emits itself**. bevy's
//! `dynamic_linking` (on for every test and bench of this crate through `[dev-dependencies.bevy]`,
//! D-22) links **std itself** as a shared object, and the allocator shim is emitted there:
//!
//! ```text
//! $ nm -D target/debug/.../test_bench_allocations-* | grep ___rust_alloc
//!                  U _RNvCs21tyadcnGr3_7___rustc12___rust_alloc
//! $ ldd <same binary> | grep libstd
//!     libstd-530112f2fd58de10.so => not found
//! ```
//!
//! Anything that runs inside `libstd-<hash>.so` or `libbevy_dylib.so` — including the generics this
//! crate instantiates — allocates through the shim compiled into that object and never reaches this
//! one. Measured in a scratch copy of `tests/test_bench_allocations.rs` with the wrapper installed: a
//! `Box::new([3u8; 8192])`, which the binary emits itself, gave `Counts { allocs: 1, bytes: 8192 }`,
//! while `vec![0u8; 4096]`, `(0..4096u64).collect::<Vec<u64>>()` and a `HashSet<u64>` insert — the
//! shapes of both known sites above — each gave `Counts { allocs: 0, bytes: 0 }`, and so did [`probe`].
//! That is how this build is known to be in the **blind** state; the bench target links the same
//! `[dev-dependencies.bevy]`, and what says so for a given run is the one [`calibration_line`] it
//! prints, not a claim written here. [`line`] and [`pair_line`] take that result and refuse every
//! verdict when the probe counted nothing. Leaving the blind state is the manifest edit D-22 records
//! (a bench bevy without `dynamic_linking`, in the protected `crates/vitality/Cargo.toml`, owner's call).
//!
//! What the gate can reach without any of that is the wrapper body, through [`ALLOC`]: calling
//! `CountingAlloc::alloc`/`realloc`/`dealloc` directly is the exact code the shim runs, so the gating,
//! the arithmetic and the window are pinned where a gate can see them.
//!
//! ## What a window really counts
//! The counters are process-wide and this build is `multi_threaded` (D-22), so an open window also
//! counts allocations other threads make while it is open. So:
//!
//! * a window's numbers are **whole-process counts, not per-system or per-entity ones**. Callers
//!   assert inequalities (`dirty > clean`) and never totals: `"17 allocs"` is a claim about the
//!   process during that window; `dirty > clean` is a claim about the tick.
//! * the tick itself is not parallel — `common::run_fixed_tick` is `World::run_schedule`, which has no
//!   `WorkerPool` to spawn onto — so a tick window is its own thread plus whatever else was awake.
//!
//! ## Windows are not reentrant
//! [`counted`] shares one pair of counters with every other window, so it is **not reentrant and not
//! nestable**: an inner window's reset wipes the outer one's tally and the outer then reports the
//! inner's tail. Nothing detects that; keep one window open per thread, which
//! `tests/test_bench_allocations.rs` does with a mutex.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// What passed through the counting body during one [`counted`] window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// `alloc` + `alloc_zeroed` + `realloc` calls.
    pub allocs: u64,
    /// The bytes those calls asked for: `Layout::size()`, and `new_size` for a `realloc`. Size classes
    /// are the allocator's business and are not in this number; §3's row counts allocations, not bytes.
    pub bytes: u64,
}

impl Counts {
    /// The two numbers §3's row reads, averaged over the ticks the window covered.
    pub fn per_tick(self, ticks: u64) -> (f64, f64) {
        let ticks = ticks.max(1) as f64;
        (self.allocs as f64 / ticks, self.bytes as f64 / ticks)
    }
}

static COUNTED_ALLOCS: AtomicU64 = AtomicU64::new(0);
static COUNTED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Off until someone asks: the wrapper is installed for a whole binary the moment a target declares
/// it, so the off path has to stay as close to bare `System` as it can.
static COUNTING: AtomicBool = AtomicBool::new(false);

/// `System` plus the two counters, and nothing else: the counter block cannot allocate, so the body
/// cannot recurse into itself, and it takes no lock.
pub struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(1, layout.size() as u64);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(1, layout.size() as u64);
        unsafe { System.alloc_zeroed(layout) }
    }

    /// Counted as one allocation of `new_size`, whether the allocator moved the block or grew it in
    /// place: either way the caller ends up holding that many bytes, and a `realloc` that is not
    /// counted would hide the growth of exactly the `Vec`s this module exists to find.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(1, new_size as u64);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// One instance, so both binaries' `#[global_allocator]` and the gate test's direct calls run the same
/// body rather than lookalikes.
pub const ALLOC: CountingAlloc = CountingAlloc;

/// The only place the counters move, and the only thing this module does that is not `System`. Gated,
/// relaxed, lock-free, allocation-free.
pub fn record(allocs: u64, bytes: u64) {
    if COUNTING.load(Relaxed) {
        COUNTED_ALLOCS.fetch_add(allocs, Relaxed);
        COUNTED_BYTES.fetch_add(bytes, Relaxed);
    }
}

/// Opens or closes the counting gate and returns the state it found. Off is the default and the state
/// every unrelated test and every timed sample runs in.
pub fn counting(on: bool) -> bool {
    COUNTING.swap(on, Relaxed)
}

/// Zeroes the counters without touching the gate.
pub fn reset() {
    COUNTED_ALLOCS.store(0, Relaxed);
    COUNTED_BYTES.store(0, Relaxed);
}

/// The counters as they stand, without resetting them. After [`counting(false)]` they still hold the
/// last window's values, so a late `snapshot` is that window, not a fresh count.
pub fn snapshot() -> Counts {
    Counts {
        allocs: COUNTED_ALLOCS.load(Relaxed),
        bytes: COUNTED_BYTES.load(Relaxed),
    }
}

/// Runs `f` as a counting window: resets, runs, closes by reading the counters, and returns `f`'s
/// value beside the delta. The gate is *not* touched — open it with [`counting`] first, close it
/// afterwards — which is what lets a test show that a closed window counts nothing.
///
/// Not reentrant and not nestable; see the module docs.
pub fn counted<T>(f: impl FnOnce() -> T) -> (T, Counts) {
    reset();
    let value = f();
    (value, snapshot())
}

/// The shapes §3's row exists to price, run once so a process can say whether it can see them:
/// `collect::<Vec<_>>()` (the graph's `IntoNeighbors::neighbors`) and a `HashSet` insert plus
/// `vec![0u8; N]` (the `Changed<T>` notification). No `Box`: the compiler emits it in the calling
/// binary itself, so it is counted even in a process blind to all three of these.
pub fn probe() -> Counts {
    let ((), counts) = counted(|| {
        let collected: Vec<u64> = (0..512u64).collect();
        let mut inserted = HashSet::with_capacity(collected.len());
        for value in &collected {
            inserted.insert(*value);
        }
        let zeroed = vec![0u8; 4096];
        black_box((&collected, &inserted, &zeroed));
    });
    counts
}

/// What one [`calibrate`] run says about the process it ran in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calibration {
    /// What [`probe`] counted with the gate open.
    pub probe: Counts,
    /// `probe.allocs > 0`: this process routes the row's own shapes through the counting wrapper.
    pub measured: bool,
}

/// Runs [`probe`] once with the gate open, then puts the gate back the way it found it. A bench calls
/// this once per run and threads the result into [`line`] and [`pair_line`].
pub fn calibrate() -> Calibration {
    let found = counting(true);
    let probe = probe();
    counting(found);
    Calibration {
        probe,
        measured: probe.allocs > 0,
    }
}

/// The line a bench run prints once, saying which of the two states it is in and what each means.
pub fn calibration_line(cal: Calibration) -> String {
    format!(
        "allocs_per_tick calibration: probe {:?} over collect::<Vec<_>>() + HashSet insert + \
         vec![0u8; N] -> {}",
        cal.probe,
        if cal.measured {
            "instrumented, so every allocs/tick line below is a lower bound and never a §3 verdict"
        } else {
            "NOT MEASURED, so every allocs/tick line below reads NOT MEASURED whatever the counters say"
        }
    )
}

/// One report line per column, so the bench prints what §3 asks for and the formatting is compiled by
/// the gate rather than only by `cargo bench`.
///
/// `touched` is the row's "entities touched" denominator: the dirty entity count the column starts
/// from, read *before* the window because a resolving tick re-dirties as it goes.
///
/// `cal`, not `counts`, decides what the line may claim. A nonzero count is not a measurement: one
/// `Box` anywhere in the process moves the counters while every `collect::<Vec<_>>()` in the tick stays
/// invisible. So an uninstrumented process prints `NOT MEASURED` whatever the counters say, and an
/// instrumented one still prints a **lower bound** — the wrapper cannot see allocations made inside the
/// shared objects, so no rendering here settles §3's row.
pub fn line(id: &str, counts: Counts, ticks: u64, touched: usize, cal: Calibration) -> String {
    let (allocs, bytes) = counts.per_tick(ticks);
    let head = format!(
        "allocs_per_tick/{id}: {allocs:.2} allocs/tick, {bytes:.0} bytes/tick over {ticks} ticks; \
         entities touched {touched}"
    );
    if !cal.measured {
        return format!(
            "{head}; NOT MEASURED: the calibration probe {cal:?} counted nothing, so this process \
             does not allocate the shapes §3's row prices through the counting shim \
             (support/alloc_count.rs), and neither number above is a measurement"
        );
    }
    if touched == 0 {
        return format!(
            "{head}; allocs/entity-touched n/a: an idle tick touches no entity, so the row's \
             denominator is 0 and only the dirty column answers it"
        );
    }
    format!(
        "{head}; {:.2} allocs per entity touched is a LOWER BOUND, not a §3 verdict: the shim \
         cannot see allocations made inside libstd-<hash>.so or libbevy_dylib.so, so the real \
         figure is >= this one",
        allocs / touched as f64
    )
}

/// The §3 pin for one shape: the same world idling against the same world with one fraction dirty.
/// Printed, not asserted — a panic in a process that has built a bevy App wedges it before it can
/// report (measured four times on this very item's test binary), and a bench that hangs tells nobody
/// anything a printed `NOT MEASURED` does not.
///
/// The `dirty > clean` claim needs `cal.measured`: with the probe blind, a single block from another
/// thread would otherwise buy the row's central assumption.
pub fn pair_line(
    clean_id: &str,
    dirty_id: &str,
    clean: Counts,
    dirty: Counts,
    ticks: u64,
    touched: usize,
    cal: Calibration,
) -> String {
    let (clean_allocs, _) = clean.per_tick(ticks);
    let (dirty_allocs, _) = dirty.per_tick(ticks);
    if !cal.measured {
        return format!(
            "allocs_per_tick PIN {dirty_id} vs {clean_id}: NOT MEASURED (the calibration probe \
             {cal:?} counted nothing), so \"one dirty entity costs allocations\" is unproven in \
             this build; the two windows ({clean:?} clean, {dirty:?} dirty) are not evidence about \
             it, and the denominator the pin would be measured against is real — {touched} \
             entities touched by the dirty column, 0 by the idle one"
        );
    }
    let verdict = if dirty_allocs > clean_allocs {
        "dirty > clean, as the row assumes"
    } else {
        "dirty <= clean: the counted sites are not what a dirty tick costs"
    };
    format!(
        "allocs_per_tick PIN {dirty_id} ({dirty_allocs:.2}/tick) vs {clean_id} \
         ({clean_allocs:.2}/tick): {verdict}; whole-process windows, so a comparison only"
    )
}
