//! P-14 (BENCH-PLAN §2G): the allocation column no duration bench can produce.
//!
//! `allocs_per_tick` in `benches/bench_systems.rs` reports two numbers off a counting
//! `#[global_allocator]`, and §3's last unmeasured row — "allocations/tick > 2× entities touched" — is
//! only as trustworthy as the thing that counts. Nothing else in the tree could observe an allocation
//! (`grep -rn global_allocator crates/` was empty before this module), so this file is the instrument's
//! own calibration: the gate off must count nothing, the gate on must see a known block and see the same
//! amount twice, and the shape the bench reports against must really touch the number of entities its id
//! says it does.
//!
//! What this binary cannot pin: it installs no `#[global_allocator]` at all, and every shape the
//! calibration probe prices is allocated inside `libstd-<hash>.so`, so the probe reads `{0,0}` here and
//! these tests pin the counting **body** — `alloc_count::ALLOC`'s own `GlobalAlloc` methods, the code the
//! shim runs — rather than that a tick's allocations reach it.
//! `support/alloc_count.rs` says why (the `nm -D` / `ldd` transcript), and the calibration probe is
//! what a *run* is asked which state it is in.
//!
//! A test here that panics *after* building a bevy App wedges the whole binary: the panicking test
//! reports, and the next test deadlocks in a futex at 0 CPU seconds (reproduced four times, with and
//! without the counting allocator installed, so it is not this module's doing). If this file ever
//! "hangs" in a gate run, look for the `FAILED` line above it; there is no timeout bug here.
//!
//! The window asserts are inequalities and equalities between two windows, never absolute totals,
//! because the counters are process-wide and this build is `multi_threaded` (D-22): a window counts
//! whatever any other thread allocates while it is open.

#[path = "../benches/support/alloc_count.rs"]
mod alloc_count;
#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/floor.rs"]
mod floor;

use crate::alloc_count::{
    ALLOC, Calibration, Counts, calibrate, calibration_line, counted, counting, line, pair_line,
};
use crate::common::{HarnessWorld, WorldSpec, actor_entities, mark_dirty_fraction, seed_rng};
use crate::floor::{dirty_entity_count, prepare_floor};
use bevy::prelude::World;
use std::alloc::{GlobalAlloc, Layout};
use std::hint::black_box;
use std::sync::{Mutex, MutexGuard};

/// One pair of counters per process, so only one of these tests may run at a time. Poisoning is
/// ignored on purpose: a failed assert leaves the atomics readable, and the next test would rather
/// measure than skip.
static COUNTERS: Mutex<()> = Mutex::new(());

fn counters() -> MutexGuard<'static, ()> {
    COUNTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Opens the counting gate and puts it back on the way out, panic included: a gate left open by a
/// failing assert would make every later test of this binary count its own allocations, and libtest
/// keeps the process running after a failure.
struct Gate(bool);

impl Gate {
    fn open() -> Self {
        Self(counting(true))
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        counting(self.0);
    }
}

/// Routes one known block through the same `GlobalAlloc` body the `#[global_allocator]` shim runs: a
/// 4096-byte `alloc`, a write through all of it so the buffer is real, one `realloc` to 8192 (the growth
/// a `Vec` does), and the matching `dealloc`. Returns bytes read back out of both buffers so neither
/// allocation can be elided.
///
/// This is the strongest form of "a known Vec" this binary can offer: a `vec![0u8; 4096]` written and
/// summed back inside an open window counted `Counts { allocs: 0, bytes: 0 }` here even with the wrapper
/// installed, because the code that allocates it lives in `libstd-<hash>.so` (module docs).
fn known_vec() -> u64 {
    let small =
        Layout::from_size_align(4096, 16).expect("4096 bytes at align 16 is a valid layout");
    let grown =
        Layout::from_size_align(8192, 16).expect("8192 bytes at align 16 is a valid layout");
    unsafe {
        let first = ALLOC.alloc(small);
        assert!(
            !first.is_null(),
            "the counting wrapper's `alloc` returned null"
        );
        first.write_bytes(7, 4096);
        let head = black_box(first.read()) as u64;

        let second = ALLOC.realloc(first, small, grown.size());
        assert!(
            !second.is_null(),
            "the counting wrapper's `realloc` returned null"
        );
        black_box(&*second);
        let tail = black_box(second.read()) as u64;

        ALLOC.dealloc(second, grown);
        head + tail
    }
}

/// Gate off is the state every other test in this binary — and every timed sample of
/// `allocs_per_tick`'s own duration column — runs in, so it has to be worth exactly nothing to the
/// counters. If a counter ever moved outside the gate, this is the test that says so.
#[test]
fn counting_off_counts_nothing() {
    let _serial = counters();
    assert!(
        !counting(false),
        "the gate came in open: counting is off by default so that declaring this module changes no \
         other test's numbers, and an open gate here means something left it open",
    );

    let (read_back, counts) = counted(|| {
        // The known block, plus a `Vec` the test crate allocates for itself, so a counter that moved
        // on *any* allocation path would show up.
        let through_wrapper = known_vec();
        let filler: Vec<u64> = (0..4096).collect();
        black_box(&filler);
        through_wrapper + filler.len() as u64
    });

    assert_eq!(
        read_back,
        7 + 7 + 4096,
        "the region under the window did not run to completion"
    );
    assert_eq!(
        counts,
        Counts::default(),
        "a closed window counted {counts:?} out of a region that allocated a 4 KiB block, grew it to \
         8 KiB and then built a 32 KiB Vec: the counter is not gated, so every number \
         `allocs_per_tick` reports is the whole run rather than its own window",
    );
}

/// The gate on has to see the block the test routes through it, and see the same amount twice, or the
/// bench's allocs/tick is noise rather than a measurement.
#[test]
fn counting_on_sees_a_known_vec() {
    let _serial = counters();
    let _gate = Gate::open();

    let (read_back, first) = counted(known_vec);
    assert_eq!(
        read_back, 14,
        "the known block was not written and read back"
    );
    assert!(
        first.allocs > 0,
        "an open window saw no allocation from one `alloc` and one `realloc` through the wrapper's own \
         body, so the counting branch never runs and `allocs_per_tick` would report 0 for every shape",
    );
    assert_eq!(
        first.allocs, 2,
        "one `alloc` plus one `realloc` through the counting body recorded {first:?}: the body counts \
         a growing Vec as two allocations, and a different number means it is counting something else \
         than the calls made through it",
    );
    assert_eq!(
        first.bytes,
        4096 + 8192,
        "one `alloc` of 4096 plus one `realloc` to 8192 recorded {first:?}: bytes is the sum of the \
         sizes asked for, which is what a bytes/tick line claims",
    );

    // Also the reset pin: a `counted` that did not reset on entry would report double here.
    let (_, second) = counted(known_vec);
    assert_eq!(
        first, second,
        "two windows over the same block disagree ({first:?} vs {second:?}), so a window is not a \
         repeatable measurement: whatever differs came from another thread allocating inside one of \
         the two windows (this build is multi_threaded, D-22) and the bench's allocs/tick would carry \
         the same noise",
    );
}

/// The two calibration states the renderers are pinned with, so both are reachable with fixed inputs:
/// a process whose probe saw the row's shapes, and one that saw none.
const INSTRUMENTED: Calibration = Calibration {
    probe: Counts {
        allocs: 3,
        bytes: 24_576,
    },
    measured: true,
};
const BLIND: Calibration = Calibration {
    probe: Counts {
        allocs: 0,
        bytes: 0,
    },
    measured: false,
};

/// A nonzero counter is not a measurement. Both sites the §3 row prices are a `collect::<Vec<_>>()` and
/// a `HashSet` insert, and both can be invisible to the wrapper while one stray `Box` from any thread
/// still moves it — so what a rendered line may claim comes from the calibration probe, never from
/// `counts.allocs != 0`.
#[test]
fn a_stray_allocation_is_not_a_measurement() {
    let _serial = counters();
    let stray = Counts {
        allocs: 1,
        bytes: 8192,
    };

    let blind = line("tiny/run_fixed_tick", stray, 50, 1, BLIND);
    assert!(
        blind.contains("NOT MEASURED") && !blind.contains("allocs per entity touched"),
        "one stray 8 KiB block in a process the probe says is blind rendered as {blind}",
    );

    let pair = pair_line(
        "tiny-dirty=0/run_fixed_tick",
        "tiny-dirty=25/run_fixed_tick",
        Counts::default(),
        stray,
        50,
        1,
        BLIND,
    );
    assert!(
        pair.contains("NOT MEASURED") && !pair.contains("dirty > clean"),
        "a clean window of 0 against a dirty window of one stray block rendered as {pair}",
    );

    // The other state, with the same inputs: a number to read, and still no §3 row verdict — the shim
    // cannot see std-internal allocations, so a ratio against entities touched bounds the row only.
    let wired = line("tiny/run_fixed_tick", stray, 50, 1, INSTRUMENTED);
    assert!(
        wired.contains("LOWER BOUND")
            && !wired.contains("NOT MEASURED")
            && !wired.contains("row MET")
            && !wired.contains("row not met"),
        "an instrumented process's ratio line claimed a row verdict: {wired}",
    );

    let wired_pair = pair_line(
        "a/run_fixed_tick",
        "b/run_fixed_tick",
        Counts::default(),
        stray,
        50,
        1,
        INSTRUMENTED,
    );
    assert!(
        wired_pair.contains("dirty > clean"),
        "with the probe seeing the row's shapes, 1 dirty alloc against 0 clean ones is the row's \
         assumption and the pin has to say so: {wired_pair}",
    );

    // Which state a plain gate build is in is asked, not assumed: no wrapper here, and every shape
    // `probe` prices lives in libstd-<hash>.so, so the probe must report this process blind and leave no
    // gate open.
    let here = calibrate();
    assert!(
        !here.measured,
        "the probe counted {here:?} in a binary that installs no `#[global_allocator]`, which is the \
         state a gate build is in; installing one here is not the answer — it counts every thread, and \
         with it the probe caught a stray {{allocs: 1, bytes: 48}} from another test's bevy pool thread \
         and 2 of 3 gate-shaped runs went red",
    );
    assert!(
        !counting(false),
        "calibrate left the counting gate open, so every later test of this binary would count",
    );
    assert!(
        calibration_line(here).contains("NOT MEASURED")
            && calibration_line(INSTRUMENTED).contains("instrumented"),
        "the once-per-run line has to name the state: {} vs {}",
        calibration_line(here),
        calibration_line(INSTRUMENTED),
    );
}

/// `counting` returns the state it *found*, which is what lets [`Gate`] put it back. An implementation
/// that echoes its argument leaves the gate open for the rest of the binary after one failed assert.
#[test]
fn counting_returns_the_state_it_found() {
    let _serial = counters();
    let found = counting(true);
    assert!(
        !found,
        "counting(true) returned {found}, the state it was asked for rather than the state it found: \
         Gate stores that and hands it back on drop, so one failing assert would leave the gate open \
         for every later test in this binary",
    );
    let opened = counting(false);
    assert!(
        opened,
        "counting(true) opened the gate, so counting(false) had to return the open state it found; it \
         returned {opened}",
    );
    assert!(
        !counting(false),
        "the gate was not put back the way this binary starts",
    );
}

/// The §3 row's denominator, on the shape `allocs_per_tick` reports first: `entities touched` is
/// `floor::dirty_entity_count` read before the window, so the row is only as honest as this number.
/// The idle column of the pair must touch nothing and the dirty column exactly the actors it marked —
/// `build_app_with` returns a world dirty on *every* attribute of every actor, so "idle" has to be
/// made, not waited for (`floor::prepare_floor`).
#[test]
fn the_dirty_column_touches_the_entities_its_id_claims() {
    let _serial = counters();
    // Three attributes, two permanent effects with one modifier each, four actors, one of them dirty.
    let spec = WorldSpec {
        attributes_per_actor: 3,
        effects_per_actor: 2,
        modifiers_per_effect: 1,
        actors: 4,
        dirty_fraction: 0.25,
        ticking_fraction: 0.0,
    };
    let mut app = spec.build_app();
    prepare_floor(&mut app);
    assert_eq!(
        dirty_entity_count(app.world()),
        0,
        "the idle column of {} starts on {} dirty entities, so the tick it reports is resolving and \
         its `dirty=0` id means nothing",
        spec.shape_id(),
        dirty_entity_count(app.world()),
    );

    let pool = app.world().resource::<HarnessWorld>().attrs();
    let actors = actor_entities(app.world_mut());
    let world: &mut World = app.world_mut();

    let marked = mark_dirty_fraction(world, pool, &actors, spec.dirty_fraction, &mut seed_rng());
    assert_eq!(
        marked,
        1,
        "shape {} marks {} actor(s), not the one entity the row's denominator is quoted against",
        spec.shape_id(),
        marked,
    );
    assert_eq!(
        dirty_entity_count(world),
        marked,
        "marking {marked} actor left {} dirty entities: `dirty_entity_count` is the \"entities \
         touched\" denominator of BENCH-PLAN §3's allocations row, so it has to see exactly what was \
         marked before the tick runs",
        dirty_entity_count(world),
    );
}
