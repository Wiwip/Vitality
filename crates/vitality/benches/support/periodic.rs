#![allow(dead_code)]

//! Family E (`periodic/tick_scan`, BENCH-PLAN §2E): what a world of ticking effects costs a tick, as
//! the attribute count grows.
//!
//! The row §2E asks for is ns/tick against A on a shape with T ticking effects and about one firing
//! per tick, and the reason A belongs on that axis at all is the system's signature:
//! `init_effect_attribute` puts one `apply_periodic_effect::<T>` into `FixedUpdate` *per attribute
//! type* (`vitality_effects/src/lib.rs:76-88`), and that system's effect query is not filtered by `T`
//! — only the `modifiers: Query<&AttributeModifier<T>>` fetch is
//! (`vitality_effects/src/periodic.rs:12-30`). So every ticking effect entity is visited once per
//! registered attribute and tested with `timer.just_finished()` each time, and growth against A is the
//! same walk multiplication T2 found on the modifier path, on a second code path.
//!
//! ## The driver is `tick`, never `run_fixed_tick`
//!
//! The shape is driven with [`common::tick`] — a whole `app.update()` — and **not** with
//! [`common::run_fixed_tick`]. `run_fixed_tick` is `World::run_schedule(FixedUpdate)`: it advances no
//! clock and it does not run `tick_effect_tickers`, which lives in `Update`
//! (`vitality_effects/src/lib.rs:63`, `vitality_effects/src/timing.rs:48-55`), so no `EffectTicker`
//! ever reaches `just_finished()` and the shape fires zero times under it — `common.rs:379-385` says
//! the same thing. Do not "fix" the group back onto the cheaper driver: it would time an idle scan
//! wearing a `periodic` label, and `tests/test_bench_periodic.rs` goes red when it happens.
//!
//! A tick of this shape is also a whole fixed tick — resolver, timers, and bevy's own per-frame work
//! (`AssetPlugin` and friends included, because `app.update()` runs `Main`, not `FixedMain` alone).
//! `floor/idle_tick` and `recompute/*` time `run_fixed_tick`, the resolver and nothing else, so their
//! ns/tick and this group's are **not comparable**, and one must not be subtracted from the other to
//! "isolate" the timer cost. What *is* comparable is this group's own slope across A.
//!
//! ## Where a firing lands in the tick
//!
//! `TimeUpdateStrategy::FixedTimesteps(1)` (set by [`common::build_app_with`]) makes one
//! [`common::tick`] advance `Time` by exactly one fixed timestep (15 625 µs, bevy's default) and run
//! `FixedUpdate` once, and bevy's main schedule runs `FixedUpdate` *before* `Update` — `First,
//! PreUpdate, RunFixedMainLoop, Update, PostUpdate, Last` (`bevy_app-0.20.1/src/main_schedule.rs:225`).
//! So the `just_finished()` a timer sets in frame N's `Update` is read by `apply_periodic_effect` in
//! frame N+1's `FixedUpdate`: a firing is applied one tick after its timer wraps (measured at this tip on
//! a fresh a=1 shape: first wrap on tick 32, first value move on tick 33). Two consequences this module
//! leans on:
//!
//! * the first [`MIN_PERIOD_TICKS`] ticks after a fresh build cannot move a value, which is the
//!   non-firing window the gate test asserts; and
//! * [`floor::prepare_floor`] is only safe on a *freshly built* periodic shape. Its settle ends with an
//!   idle `run_fixed_tick`, and if a timer had just wrapped, that call would apply the firing (nothing
//!   clears `just_finished()` until the next `tick`) and its zero-dirty assert would trip. Timers are
//!   ≤ 2 timesteps old on a fresh build, so the settle is sound here — and settling a shape that has
//!   already been ticking would not be.
//!
//! ## Where T comes from, and what "about one firing per tick" means here
//!
//! [`common::prepare_world`] builds each effect as
//! `EffectBuilder::every_second_permanently(rng.random_range(0.5..1.5))` when its coin lands
//! (`common.rs:334-341`), so a period is 0.5–1.5 s = [`MIN_PERIOD_TICKS`]–[`MAX_PERIOD_TICKS`] fixed
//! ticks: one instance fires about once every 64 ticks, so "one firing per tick" is an *aggregate*
//! property of ~64 instances, not a property of any one effect.
//!
//! That coin is a per-effect flip, so [`common::WorldSpec::ticking_fraction`] does not say how many
//! instances are ticking. [`PeriodicShape::ticking`] counts `EffectTicker` components on the actor
//! instead, and the id carries that count. At [`common::SEED`], [`PERIODIC_EFFECTS`] effects and
//! [`PERIODIC_TICKING_FRACTION`] the seed gives 59 ticking and 61 permanent effects on every point of the
//! grid (measured: the same 59 at a=1, a=5 and a=20 — the attribute an amount lands on is drawn after the
//! coin, so A does not move the coin), and [`FiringStats`] then measures the rate that shape actually runs
//! at — 1.043 firings per tick at this tip — rather than the one 59/64 predicts.
//!
//! Two numbers come out of the window, because one is not enough:
//!
//! * [`FiringStats::moved_ticks`] / window — the share of ticks that moved a value. Two firings landing
//!   in one tick count once, so it *under*-states the rate and saturates as the rate grows;
//! * [`FiringStats::fired_per_tick`] — Σ value delta ÷ (window × mean firing amount), the mean amount
//!   read from [`HarnessWorld::plan`]'s ticking entries. Every modifier here is `ModOp::Add` with an
//!   amount in 1.0–2.0 and every ticking effect carries exactly one of them, so the delta is linear in
//!   the number of firings and does not saturate.
//!
//! The bands use both: the move share catches "nothing fires" (a frozen clock, or every effect
//! permanent), the linear one catches "twice as many firings as the row claims" (every effect ticking).
//! What neither can do is tell two firings on one attribute in one tick from one firing of double the
//! amount — see the gate test's doc.

use crate::common::{
    ATTRIBUTE_POOL, EffectPlan, HarnessWorld, WorldSpec, actor_entities, build_app_with,
    current_values, tick,
};
use crate::floor;
use bevy::prelude::*;
use std::time::Duration;
use vitality::effect::{AppliedEffects, EffectTicker};

/// The A axis of §2E: attribute types registered, i.e. how many copies of `apply_periodic_effect` walk
/// the effect entities. The same three points `recompute/*` uses, and 20 is the deepest the default
/// pool serves without declaring a second pool (`common.rs`).
pub const PERIODIC_ATTRS: [usize; 3] = [1, 5, 20];

/// Effects built per actor. The coin lands on roughly half of them, so this is about twice the T the
/// row wants; the permanent half is inside the id's `e=` and `tick=` fields, and inside every ns/tick
/// below — see [`PERIODIC_TICKING_FRACTION`] for why the shape cannot ask for ticking effects directly.
pub const PERIODIC_EFFECTS: usize = 120;

/// The per-effect coin [`common::prepare_world`] flips. Half the effects come out ticking, which makes
/// "every effect ticking" — §2E's `periodic/all_fire` burst — a shape that differs from this one by a
/// factor of two in the firing rate, and the gate test's band is what keeps the two apart.
pub const PERIODIC_TICKING_FRACTION: f32 = 0.5;

/// Actors per shape. One, so that "T ticking effects at about one firing per tick" and "A attributes"
/// are statements about the same single actor; a second actor would double the aggregate rate while
/// leaving A — the thing the row varies — alone.
pub const PERIODIC_ACTORS: usize = 1;

/// bevy's `Time::<Fixed>::DEFAULT_TIMESTEP` (`bevy_time-0.20.1/src/fixed.rs:76`), which
/// `TimeUpdateStrategy::FixedTimesteps(1)` charges to `Time` on every [`common::tick`]. Asserted
/// against the live resource in [`periodic_shape`], because the two tick counts below are derived from
/// it.
pub const FIXED_TIMESTEP_MICROS: u64 = 15_625;

/// The shortest period the generator draws is 0.5 s, so no instance can wrap before this many ticks.
pub const MIN_PERIOD_TICKS: usize = 32;

/// And the longest is 1.5 s, so this many ticks bounds the longest period, not the second firing: with
/// T=59 the first 96 ticks hold 75.7 firings, because the instances on the 32-tick period wrap again at
/// 64. Nothing about the value changes for [`WARM_UP_TICKS`], which only needs every instance to have
/// had its chance to fire once.
pub const MAX_PERIOD_TICKS: usize = 96;

/// Ticks of the non-firing window the gate test reads: inside [`MIN_PERIOD_TICKS`], with the one-tick
/// apply lag of the module doc on top.
pub const QUIET_TICKS: usize = 16;

/// Ticks run before the rate window opens, so the window is steady state rather than the cold start:
/// past [`MAX_PERIOD_TICKS`], i.e. every instance has had its chance to fire once.
pub const WARM_UP_TICKS: usize = 128;

/// Ticks in the rate window. Long enough that the ~64-instance beat averages out, short enough that a
/// probe is milliseconds.
pub const RATE_TICKS: usize = 512;

/// Band for [`FiringStats::fired_per_tick`], the linear measure. The row claims ~1 firing per tick and
/// measures 1.043 at this tip; the upper bound is what "every effect ticking" trips (that shape measures
/// 2.061 firings per tick), and the lower bound is what a frozen clock or a world of permanent effects
/// trips (both measure 0.0).
pub const FIRED_PER_TICK_BAND: (f64, f64) = (0.55, 1.55);

/// Band for [`FiringStats::moved_ticks`] / window, the saturating measure — 0.658 at this tip (337 of 512
/// ticks). It excludes the 0.0 of a shape that never fires, and a doubled shape too, though there the
/// linear band is the one that trips first; what it cannot exclude is a shape that fires 1.5× too often,
/// because a share cannot see two firings landing in one tick.
pub const MOVING_TICK_FRACTION_BAND: (f64, f64) = (0.40, 0.78);

/// The shape §2E times: one actor, [`PERIODIC_EFFECTS`] effects of one modifier each, about half of
/// them periodic. `dirty_fraction` is 0 because [`floor::prepare_floor`] settles the world, and
/// [`periodic_id`] replaces that field with the set the timed ticks really see.
pub fn periodic_spec(attributes: usize) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: PERIODIC_EFFECTS,
        modifiers_per_effect: 1,
        actors: PERIODIC_ACTORS,
        dirty_fraction: 0.0,
        ticking_fraction: PERIODIC_TICKING_FRACTION,
    }
}

/// A built, settled periodic shape, ready for the timed loop. Held whole: the bench times
/// [`common::tick`] on `app`, so the `App` has to outlive the closure.
pub struct PeriodicShape {
    pub app: App,
    pub spec: WorldSpec,
    /// The one actor the row varies A on.
    pub actor: Entity,
    /// Every actor the shape spawned, so a guard sees an extra spawn rather than reading past it.
    pub actors: Vec<Entity>,
    /// T: effect instances on [`PeriodicShape::actor`] carrying an `EffectTicker`, counted off the
    /// world — not the plan's ticking count, and not `ticking_fraction × effects`.
    pub ticking: usize,
    /// `Dirty<T>` components [`floor::prepare_floor`] removed. Zero would mean the settle was reading a
    /// world the walk never saw a marker in.
    pub markers_cleared: usize,
}

/// Builds `spec` from the default pool and settles it. Safe to settle because the build is fresh:
/// `build_app_with` runs one `update` and [`floor::prepare_floor`] one more, so no timer is older than
/// two timesteps and none can have wrapped — see the module doc for what a stale `just_finished()`
/// would do to that settle.
pub fn periodic_shape(spec: &WorldSpec) -> PeriodicShape {
    let mut app = build_app_with(spec, ATTRIBUTE_POOL);
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        spec.actors,
        "shape {} built {} actors, expected {}",
        spec.shape_id(),
        actors.len(),
        spec.actors,
    );
    let step = app.world().resource::<Time<Fixed>>().timestep();
    assert_eq!(
        step,
        Duration::from_micros(FIXED_TIMESTEP_MICROS),
        "shape {} runs on a {} µs fixed timestep, so MIN_PERIOD_TICKS/MAX_PERIOD_TICKS ({}–{} ticks) \
         are the wrong window for a 0.5–1.5 s period band",
        spec.shape_id(),
        step.as_micros(),
        MIN_PERIOD_TICKS,
        MAX_PERIOD_TICKS,
    );
    let markers_cleared = floor::prepare_floor(&mut app);
    let actor = actors[0];
    let ticking = ticking_instances(app.world(), actor);
    let planned = app
        .world()
        .resource::<HarnessWorld>()
        .plan
        .iter()
        .filter(|effect| effect.ticking)
        .count();
    assert_eq!(
        ticking,
        planned,
        "shape {}'s plan says {} ticking effects and the actor carries {} EffectTicker instances: the \
         id's periodic= field and the rate measured below would be about different worlds",
        spec.shape_id(),
        planned,
        ticking,
    );
    assert!(
        ticking > 0,
        "shape {} produced no ticking effect at all, so there is no periodic scan to time and \
         `periodic/tick_scan` would be a second copy of `floor/idle_tick`",
        spec.shape_id(),
    );
    PeriodicShape {
        app,
        spec: spec.clone(),
        actor,
        actors,
        ticking,
        markers_cleared,
    }
}

/// Effect instances on `actor` carrying an `EffectTicker`, i.e. the ones a tick can fire. Counted
/// through the actor's own `AppliedEffects`, so a ticking effect that never landed on the actor is not
/// counted and the number is not a restatement of the plan.
pub fn ticking_instances(world: &World, actor: Entity) -> usize {
    let applied = world.get::<AppliedEffects>(actor).unwrap_or_else(|| {
        panic!(
            "actor {actor:?} carries no AppliedEffects, so there is nothing to count ticking effects on"
        )
    });
    applied
        .iter()
        .filter(|effect| world.get::<EffectTicker>(*effect).is_some())
        .count()
}

/// The youngest ticking timer on `actor`, which is what proves a window is running rather than empty:
/// if the smallest `elapsed` grew, every ticker advanced.
pub fn min_ticker_elapsed(world: &World, actor: Entity) -> Duration {
    let applied = world
        .get::<AppliedEffects>(actor)
        .expect("actor carries AppliedEffects");
    let mut youngest: Option<Duration> = None;
    for effect in applied.iter() {
        let Some(ticker) = world.get::<EffectTicker>(effect) else {
            continue;
        };
        youngest = Some(youngest.map_or(ticker.elapsed(), |min| min.min(ticker.elapsed())));
    }
    youngest
        .unwrap_or_else(|| panic!("actor {actor:?} carries no ticking effect to read a timer off"))
}

/// The non-firing window: `tick` a shape `ticks` times and count the ticks that moved a value.
pub struct QuietWindow {
    pub ticks: usize,
    /// Ticks that moved at least one attribute. 0 is the claim.
    pub moved_ticks: usize,
    /// Ticking instances on the actor, so the window cannot be empty by accident.
    pub ticking: usize,
    pub elapsed_before: Duration,
    pub elapsed_after: Duration,
    /// The actor's attribute values when the window opened, and when it closed: equal is the claim.
    pub values_before: Vec<f64>,
    pub values_after: Vec<f64>,
}

pub fn quiet_window(shape: &mut PeriodicShape, ticks: usize) -> QuietWindow {
    let actor = shape.actor;
    let pool = shape.app.world().resource::<HarnessWorld>().attrs();
    let values_before = current_values(shape.app.world(), pool, actor);
    let elapsed_before = min_ticker_elapsed(shape.app.world(), actor);
    let ticking = ticking_instances(shape.app.world(), actor);

    let mut moved_ticks = 0;
    let mut previous = values_before.clone();
    for _ in 0..ticks {
        tick(&mut shape.app);
        let now = current_values(shape.app.world(), pool, actor);
        if now != previous {
            moved_ticks += 1;
        }
        previous = now;
    }
    let values_after = previous;

    QuietWindow {
        ticks,
        moved_ticks,
        ticking,
        elapsed_before,
        elapsed_after: min_ticker_elapsed(shape.app.world(), actor),
        values_before,
        values_after,
    }
}

/// The rate window: ticks of the shape read as applied results.
pub struct FiringStats {
    pub window: usize,
    pub warm_up: usize,
    /// Ticks that moved a value. Saturating: two firings in one tick count once.
    pub moved_ticks: usize,
    /// Σ attribute delta over the window, in value units.
    pub total_delta: f64,
    pub delta_per_tick: f64,
    /// Mean `Add` amount of the ticking effects — the value units one firing of this shape moves.
    pub mean_firing_amount: f64,
    /// `delta_per_tick / mean_firing_amount`: firings per tick, linear in the firing count.
    pub fired_per_tick: f64,
}

/// Runs [`WARM_UP_TICKS`] and then a `window`-tick window on `shape`. This uses the shape's clock up,
/// so it runs on a probe world and never on the world the bench times.
pub fn firing_stats(shape: &mut PeriodicShape, window: usize) -> FiringStats {
    let actor = shape.actor;
    let pool = shape.app.world().resource::<HarnessWorld>().attrs();
    let plan = shape.app.world().resource::<HarnessWorld>().plan.clone();
    let mean_firing_amount = mean_firing_amount(&plan);

    for _ in 0..WARM_UP_TICKS {
        tick(&mut shape.app);
    }

    let mut previous = current_values(shape.app.world(), pool, actor);
    let mut moved_ticks = 0;
    let mut total_delta = 0.0;

    for _ in 0..window {
        tick(&mut shape.app);
        let now = current_values(shape.app.world(), pool, actor);
        let mut moved = false;
        for (before, after) in previous.iter().zip(&now) {
            let delta = after - before;
            assert!(
                delta >= 0.0,
                "an attribute of shape {} moved by {delta} in one tick: every modifier here is \
                 `ModOp::Add`, so a negative step means the window is not a firing count any more",
                shape.spec.shape_id(),
            );
            if delta > 0.0 {
                moved = true;
            }
            total_delta += delta;
        }
        if moved {
            moved_ticks += 1;
        }
        previous = now;
    }

    let delta_per_tick = total_delta / window as f64;
    FiringStats {
        window,
        warm_up: WARM_UP_TICKS,
        moved_ticks,
        total_delta,
        delta_per_tick,
        mean_firing_amount,
        fired_per_tick: delta_per_tick / mean_firing_amount,
    }
}

/// A shape plus the two windows the group's claims rest on. The probe world's clock is used up by
/// reading them, so the world the bench times is a fresh [`periodic_shape`] of the same spec — and the
/// gate test asserts the two agree on T.
pub struct Probe {
    pub shape: PeriodicShape,
    pub quiet: QuietWindow,
    pub firing: FiringStats,
}

pub fn probe(spec: &WorldSpec) -> Probe {
    let mut shape = periodic_shape(spec);
    let quiet = quiet_window(&mut shape, QUIET_TICKS);
    let firing = firing_stats(&mut shape, RATE_TICKS);
    Probe {
        shape,
        quiet,
        firing,
    }
}

/// Mean `Add` amount over the ticking effects' modifiers: the value units one firing of this shape
/// moves. Every ticking effect has to carry exactly one modifier, because the estimator divides the
/// window's delta by this number and calls the quotient a firing count.
pub fn mean_firing_amount(plan: &[EffectPlan]) -> f64 {
    let ticking: Vec<&EffectPlan> = plan.iter().filter(|effect| effect.ticking).collect();
    assert!(
        !ticking.is_empty(),
        "no ticking effect in the plan, so there is no firing amount to divide a window's delta by",
    );
    let amounts: Vec<f64> = ticking
        .iter()
        .flat_map(|effect| effect.modifiers.iter().map(|m| m.amount as f64))
        .collect();
    assert_eq!(
        amounts.len(),
        ticking.len(),
        "{} ticking effects carry {} modifiers between them: the shape is m=1, and the estimator only \
         divides a delta by a mean amount when each firing moves exactly one amount",
        ticking.len(),
        amounts.len(),
    );
    amounts.iter().sum::<f64>() / amounts.len() as f64
}

/// The id §2E prints: [`common::WorldSpec::shape_id`] with the things it cannot say carried as
/// suffixes, the way `cross_actor.rs` carries `readers=` (a `WorldSpec` field is not an option — a
/// foreign test writes full struct literals of it):
///
/// * `dirty=all` replaces `dirty=0`, in the same move `recompute::dirtied_id` makes. The world is
///   settled to zero dirty at setup, and a resolve re-marks what it resolved (`recompute.rs`), so the
///   dirty set ramps up rather than decaying — measured first full set on tick 33 (A=1), 43 (A=5), 85
///   (A=20) — so `all` is the post-first-firing steady state nearly every sample starts on, not the
///   world's starting state. The ramp is accumulation, not spread: marks do not move sideways onto
///   attributes that never fired, which is what B-4's landed `marks_never_decay_and_never_spread`
///   asserts. Both places that use the claim re-read the set after the fact — the gate test at the end
///   of its firing window, the group's post-loop guard at the end of the timed loop;
/// * `app_update` names the driver, because the group's premise is that it is not `run_fixed_tick`;
/// * `periodic=T` and `fire=X.XX` are the measured T and the measured firing rate of this exact shape;
/// * `mt-dyn` is the build every number here comes from (D-22).
pub fn periodic_id(spec: &WorldSpec, ticking: usize, firing: &FiringStats) -> String {
    let plain = spec.shape_id();
    let id = plain.replace("/dirty=0", "/dirty=all");
    assert!(
        id != plain,
        "shape {plain} carries no /dirty= field to replace, so the dirty set the timed ticks really \
         see cannot be put in its id",
    );
    format!(
        "{id}/app_update/periodic={ticking}/fire={:.2}/mt-dyn",
        firing.fired_per_tick,
    )
}

/// One measured point printed beside its id, so a bench run ships the rate the id quotes next to the
/// ns/tick line criterion prints for it.
pub fn point_line(spec: &WorldSpec, shape: &PeriodicShape, firing: &FiringStats) -> String {
    format!(
        "{} fired={:.3}/tick moved={}/{} ticks ({:.3}) delta={:.3}/tick over {} ticks after a \
         {}-tick warm-up, T={}, markers_cleared={}",
        periodic_id(spec, shape.ticking, firing),
        firing.fired_per_tick,
        firing.moved_ticks,
        firing.window,
        firing.moved_ticks as f64 / firing.window as f64,
        firing.delta_per_tick,
        firing.window,
        firing.warm_up,
        shape.ticking,
        shape.markers_cleared,
    )
}
