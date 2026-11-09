//! B-3 (BENCH-PLAN §2E): the `periodic/tick_scan` shape really fires about one effect per tick, and
//! only when the shape is driven by `common::tick` — a whole `app.update()`.
//!
//! The claim is checked on *applied results*: the actor's attribute values are read across windows of
//! `common::tick` calls and the ticks that moved a value are counted. Nothing here reads
//! `WorldSpec::ticking_fraction` as evidence — that fraction is a per-effect coin flip in
//! `common::prepare_world`, so it says what the shape asked for, not what it got and not what fired.
//! [`periodic::FiringStats`] is the measure, and the bands it is checked against live in
//! `benches/support/periodic.rs` next to the group that quotes the numbers.
//!
//! Four things are pinned, each a way the group could silently become a different bench:
//!
//! * **the rate**: `fired_per_tick` sits in the band around 1.0 at the shallowest and deepest A of the
//!   grid, so the row is "T ticking effects at about one firing per tick" and not "an idle scan" and
//!   not "everything fires at once";
//! * **the driver**: `fired_per_tick` and the timer advance both come out of `tick`, which is the only
//!   thing that moves `Time<Fixed>` and `tick_effect_tickers`. Swap the group's driver for
//!   `common::run_fixed_tick` and both asserts below go red — that is the item's premise;
//! * **a non-firing window exists and is real**: the first ticks after the shape settles move nothing,
//!   because the shortest period the generator draws (0.5 s) is [`periodic::MIN_PERIOD_TICKS`] fixed
//!   ticks. The window is proved live rather than empty — the ticking instances are on the actor, their
//!   timers advanced across it, and the same world fires the moment the window ends;
//! * **the id means what it says**: `T` is counted off `EffectTicker` entities on the actor, and the
//!   dirty set after the firing window is the `/dirty=all` the id claims.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/floor.rs"]
mod floor;
#[path = "../benches/support/periodic.rs"]
mod periodic;

use crate::common::HarnessWorld;
use crate::periodic::{FIRED_PER_TICK_BAND, MOVING_TICK_FRACTION_BAND, PERIODIC_ATTRS};

/// The grid ends this test checks: the shallowest and the deepest A the group times. Built from the
/// grid itself, so this cannot drift from the bench's x-axis.
const GUARD_ATTRS: [usize; 2] = [PERIODIC_ATTRS[0], PERIODIC_ATTRS[PERIODIC_ATTRS.len() - 1]];

#[test]
fn periodic_tick_scan_fires_about_once_per_tick() {
    assert!(
        GUARD_ATTRS.iter().all(|a| PERIODIC_ATTRS.contains(a)),
        "the guard shape {GUARD_ATTRS:?} is no longer a point of the grid the bench times \
         ({PERIODIC_ATTRS:?}), so this file would be guarding a shape no row prints",
    );

    for attributes in GUARD_ATTRS {
        let spec = periodic::periodic_spec(attributes);
        let probed = periodic::probe(&spec);
        let shape = &probed.shape;
        let world = shape.app.world();
        let pool = world.resource::<HarnessWorld>().attrs();

        // The shape is what its id says it is: one actor, one modifier per effect, and T counted off
        // the world rather than off the plan.
        assert_eq!(
            pool.len(),
            attributes,
            "shape {} registered {} attributes, not the {} its a= field names",
            spec.shape_id(),
            pool.len(),
            attributes,
        );
        assert_eq!(
            shape.actors.len(),
            1,
            "shape {} built {} actors: the row prices T ticking effects on one actor, so a second \
             actor would double the firing rate the id's fire= field quotes",
            spec.shape_id(),
            shape.actors.len(),
        );
        assert!(
            shape.markers_cleared > 0,
            "settling shape {} cleared no Dirty<T>, so the walk that reports a settled world never \
             saw one and the quiet-window assert below proves nothing",
            spec.shape_id(),
        );
        assert!(
            shape.ticking > 0,
            "shape {} carries no EffectTicker at all, so there is no periodic scan to time and \
             `periodic/tick_scan` would be a second copy of `floor/idle_tick`",
            spec.shape_id(),
        );
        assert_eq!(
            probed.quiet.ticking, shape.ticking,
            "the probe counted {} ticking instances and the shape reports {}: the rate below would \
             be measured on a different world from the one the group times",
            probed.quiet.ticking, shape.ticking,
        );

        // A non-firing window — and the proof it is a window and not a dead world.
        let quiet = &probed.quiet;
        assert_eq!(
            quiet.moved_ticks,
            0,
            "{} of the first {} ticks of shape {} moved a value: the shortest period the generator \
             draws is 0.5 s = {} fixed ticks, so something fired that early and the quiet window is \
             not quiet",
            quiet.moved_ticks,
            quiet.ticks,
            spec.shape_id(),
            periodic::MIN_PERIOD_TICKS,
        );
        assert!(
            quiet.elapsed_after > quiet.elapsed_before,
            "no ticking timer advanced across the {} quiet ticks of shape {} ({} -> {} s): that \
             window is empty rather than non-firing, which is what `run_fixed_tick` does to this \
             shape, so the group would be timing a frozen clock",
            quiet.ticks,
            spec.shape_id(),
            quiet.elapsed_before.as_secs_f64(),
            quiet.elapsed_after.as_secs_f64(),
        );
        assert_eq!(
            quiet.values_after,
            quiet.values_before,
            "shape {} reads {:?} at the end of its quiet window and {:?} at its start, even though no \
             tick of that window moved a value",
            spec.shape_id(),
            quiet.values_after,
            quiet.values_before,
        );

        let firing = &probed.firing;
        assert!(
            firing.moved_ticks > 0,
            "shape {} moved no attribute value in {} ticks after its quiet window: the world that was \
             quiet above is inert, so the rate below is 0 by construction rather than because a clock \
             is frozen — and `all_fire`/`tick_scan` would both be unmeasurable",
            spec.shape_id(),
            firing.window,
        );

        // Non-vacuity: a shape that moves a value on *every* tick is "an effect fires on every tick",
        // which is not what the group's name claims and is the state where the move counter below
        // stops carrying information.
        assert!(
            firing.moved_ticks < firing.window,
            "every one of shape {}'s {} timed ticks moved a value, so the shape is \"an effect fires \
             on every tick\", not the \"about one firing per tick\" the group's name claims: the \
             per-tick move counter saturates and can no longer locate the firings",
            spec.shape_id(),
            firing.window,
        );

        let moving_fraction = firing.moved_ticks as f64 / firing.window as f64;
        // Two bands, because one measure cannot carry the claim alone. The move share saturates (two
        // firings in one tick count once), so it says "nothing fires" but not "twice as much as
        // claimed"; the linear one is the firings-per-tick figure the id quotes. It is measured on value
        // deltas and divided by the plan's mean firing amount, which is a unit of account, not the
        // claim: nothing here reads `ticking_fraction` and calls it a firing rate.
        assert!(
            (FIRED_PER_TICK_BAND.0..=FIRED_PER_TICK_BAND.1).contains(&firing.fired_per_tick),
            "shape {} fires {:.3} effects per tick by its own value deltas, outside the [{}, {}] \
             band, so the id's fire= field and BENCH-PLAN §2E's \"~1 firing per tick\" are both wrong \
             for T = {} ticking effects (delta/tick {:.3}, mean firing amount {:.3})",
            spec.shape_id(),
            firing.fired_per_tick,
            FIRED_PER_TICK_BAND.0,
            FIRED_PER_TICK_BAND.1,
            shape.ticking,
            firing.delta_per_tick,
            firing.mean_firing_amount,
        );
        assert!(
            (MOVING_TICK_FRACTION_BAND.0..=MOVING_TICK_FRACTION_BAND.1).contains(&moving_fraction),
            "shape {} moved a value on {:.3} of its {} ticks, outside the [{}, {}] band the \
             tick_scan row is quoted with: T = {} ticking effects is not about-one-firing-per-tick",
            spec.shape_id(),
            moving_fraction,
            firing.window,
            MOVING_TICK_FRACTION_BAND.0,
            MOVING_TICK_FRACTION_BAND.1,
            shape.ticking,
        );

        // The `/dirty=all` the id claims: a resolve re-marks what it resolved (`common.rs` and
        // `recompute.rs` say why), so the dirty set ramps up and stays — first full set on tick 33/43/85
        // for A=1/5/20. A world that ends clean means the firing window never dirtied anything.
        let dirty: Vec<&str> = pool
            .iter()
            .filter(|attr| attr.is_dirty(world, shape.actor))
            .map(|attr| attr.name)
            .collect();
        assert_eq!(
            dirty.len(),
            pool.len(),
            "shape {} ends its firing window dirty on {dirty:?}, not on all {} of its attributes: \
             the id says dirty=all, so most of the timed ticks would be idling",
            spec.shape_id(),
            pool.len(),
        );

        println!(
            "periodic/tick_scan {} T={} quiet={}/{} ticks (timers {} -> {} s) moved={}/{} ({:.3}) \
             fired={:.3}/tick delta={:.3}/tick id={}",
            spec.shape_id(),
            shape.ticking,
            quiet.moved_ticks,
            quiet.ticks,
            quiet.elapsed_before.as_secs_f64(),
            quiet.elapsed_after.as_secs_f64(),
            firing.moved_ticks,
            firing.window,
            moving_fraction,
            firing.fired_per_tick,
            firing.delta_per_tick,
            periodic::periodic_id(&spec, shape.ticking, firing),
        );
    }
}
