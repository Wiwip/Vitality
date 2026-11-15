//! B-1: what one registered attribute costs the app, in structural counts rather than durations, so
//! the gate can pin it without a stopwatch. Measured at this tip (bevy 0.20.1) on the per-attribute
//! app `measure` builds: `FixedUpdate` `systems_len()` is **24 at 2 attributes and 144 at 22 → 6
//! scheduled nodes per attribute**, and `world.components().len()` is **152 at 2 and 372 at 22 → 11
//! components per attribute**. Both baselines are exactly those measured slopes — no headroom, so the
//! next change to `init_core_attribute` moves a number here rather than a guess.
//!
//! How to read the intercept: `measure` runs a single `app.update()`, and there bevy primes its clock
//! and returns a zero delta, so no fixed step runs, `FixedUpdate`'s executor is never built and
//! `systems_len()` still reports the graph rather than the compiled schedule. The 12 below is that
//! graph's fixed half, `12 + 6a`; a `measure` that really ticks has to re-measure it.
//!
//! Slope and intercept are pinned separately because neither sees the other: one node added at plugin
//! build time leaves `(145 - 25) / 20` at 6, and a change that only widens the per-attribute cost
//! leaves the `a=2` count alone. Together they hold both halves of `cost = intercept + slope·A`.
//!
//! Every bound here is one-sided, so getting cheaper never goes red: a leaner `init_core_attribute`
//! passes silently, and whoever cuts registration cost owns lowering these baselines in the same
//! commit. The fixed half is pinned for `systems_len()` only — the fixed part of `components().len()`
//! (130 of that app's 152 at two attributes) is `MinimalPlugins`/`AssetPlugin` infrastructure, which moves
//! with the harness plugin set for reasons this file has nothing to do with. The four DAG counters are
//! the exception: exact numbers, so a leaner DAG has to re-measure them in the same commit too.
//!
//! The shape is 1 actor, 0 effects and 0 modifiers on purpose: this prices *registration*, and every
//! per-actor or per-effect component would be charged into the per-attribute slope. Both widths come
//! from the default 24-deep [`common::ATTRIBUTE_POOL`], so no new attribute types are declared here.
//!
//! The third §2G counter — `AppTypeRegistry` registrations, 58 at 2 and 138 at 22 (4 per
//! attribute) — is printed, never asserted: reflection entries come from every registered type in the
//! app, `MinimalPlugins`/`AssetPlugin` included, so they move with unrelated dev-dependencies and a
//! bound on them would go red for reasons this file has nothing to do with.

#[path = "../benches/support/common.rs"]
mod common;

use crate::common::{ATTRIBUTE_POOL, WorldSpec, build_app_with};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

/// The two widths the slope is taken between. 22 is the widest the default pool can serve beside 2.
const NARROW: usize = 2;
const WIDE: usize = 22;

/// Attributes between the two builds, i.e. the divisor of every slope here.
const STEP: usize = WIDE - NARROW;

/// The measured slopes of the app [`build_app_with`] builds, so a regression is red rather than
/// merely slower. Measured values are printed in the assert messages too.
const SYSTEMS_PER_ATTR_BASELINE: usize = 6;
// 10 -> 11: the attribute registry pre-registers ClampBounds<T>
const COMPONENTS_PER_ATTR_BASELINE: usize = 11;

/// Fixed `FixedUpdate` nodes at zero attributes, i.e. the measured `24 - 2 * 6` of the graph that
/// never runs. The companion of the slope above: a node added at plugin build time is a +1 here.
/// 8 → 12 when the effect tickers, durations, stack events and conditions moved off `Update`.
const SYSTEMS_INTERCEPT_BASELINE: usize = 12;

/// The DAG app all four §2G DAG counters read: the harness plugin set, the mode the collapsed nodes need,
/// and `attributes` attributes registered through the same pool entries the bench shapes use.
fn dag_app(attributes: usize) -> App {
    let mut app = App::new();
    app.insert_resource(vitality::ResolverMode::Dag);
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        vitality::AttributesPlugin,
    ));
    for attr in &ATTRIBUTE_POOL[..attributes] {
        (attr.register)(&mut app);
    }
    app
}

/// Every scheduled node [`dag_app`] owns at [`NARROW`] attributes, summed over all its schedules.
/// Exact, both directions: the fourth once-per-app node `EffectsPlugin`'s DAG branch would add is a
/// +1 here.
const DAG_SYSTEMS_BASELINE: usize = 46;

/// Observers [`dag_app`] carries at [`NARROW`] attributes: the once-per-app ones and none per
/// attribute. An observer is not a schedule node, so only this counter can see one.
const DAG_OBSERVERS_BASELINE: usize = 12;

/// Resources a ticked [`dag_app`] holds: **71 at [`NARROW`] and 151 at [`WIDE`], 4 per attribute** —
/// the message resource each registered attribute brings at rest, plus `BaseScanState<T>`,
/// `ApplyState<T>` and `PeriodicState<T>`, which the collapsed nodes only install on first use.
/// Those last three are what a counter reading the app at rest cannot see.
const DAG_TICK_RESOURCES_BASELINE: usize = 71;
const DAG_TICK_RESOURCES_PER_ATTRIBUTE: usize = 4;

/// Components a ticked [`dag_app`] has registered, same two halves: **152 at [`NARROW`] and 352 at
/// [`WIDE`], 10 per attribute**. Exact, so a component the DAG branch starts registering per attribute
/// is red here. The per-attribute app `measure` builds reads 152 at [`NARROW`] too, but on a slope of
/// 11 that reaches 372 at [`WIDE`]: two different apps that happen to cross at [`NARROW`], so a
/// figure from one never checks a claim about the other.
const DAG_TICK_COMPONENTS_BASELINE: usize = 152;
const DAG_TICK_COMPONENTS_PER_ATTRIBUTE: usize = 10;

fn spec(attributes: usize) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: 0,
        modifiers_per_effect: 0,
        actors: 1,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// The three §2G counters, read off a built harness app.
#[derive(Clone, Copy, Debug)]
struct Counters {
    systems: usize,
    components: usize,
    reflected: usize,
}

fn measure(attributes: usize) -> Counters {
    let app = build_app_with(&spec(attributes), ATTRIBUTE_POOL);
    let schedule = app
        .get_schedule(FixedUpdate)
        .expect("the harness app has no FixedUpdate schedule to count systems in");
    let reflected = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("no AppTypeRegistry resource")
        // No len() on TypeRegistry in bevy 0.20; iter() is its registration count.
        .read()
        .iter()
        .count();
    let counters = Counters {
        systems: schedule.systems_len(),
        components: app.world().components().len(),
        reflected,
    };
    println!("B-1 a={attributes}: {counters:?}");
    counters
}

/// Per-attribute cost between the two builds. Registering an attribute cannot remove a node or a
/// component, so a negative delta is a broken counter rather than a cheap attribute.
fn slope(from: usize, to: usize, counter: &str) -> usize {
    assert!(
        to >= from,
        "{counter} shrank from {NARROW} to {WIDE} attributes: {from} -> {to}",
    );
    (to - from) / STEP
}

#[test]
fn registration_cost_per_attribute_is_pinned() {
    let narrow = measure(NARROW);
    let wide = measure(WIDE);

    let systems = slope(narrow.systems, wide.systems, "FixedUpdate systems_len()");
    let components = slope(narrow.components, wide.components, "components().len()");

    assert!(
        systems <= SYSTEMS_PER_ATTR_BASELINE,
        "measured {systems} scheduled nodes per attribute: FixedUpdate systems_len() is {} at \
         {NARROW} attributes and {} at {WIDE}; baseline is {SYSTEMS_PER_ATTR_BASELINE} \
         (the floor report's 6 nodes per attribute)",
        narrow.systems,
        wide.systems,
    );
    // The companion pin: an extra node added at plugin build time is +1 at both widths, so the slope
    // above still reads 6 (25 at 2, 145 at 22) and only this intercept moves.
    let intercept = narrow
        .systems
        .saturating_sub(SYSTEMS_PER_ATTR_BASELINE * NARROW);
    assert!(
        intercept <= SYSTEMS_INTERCEPT_BASELINE,
        "measured {intercept} fixed scheduled nodes: FixedUpdate systems_len() is {} at {NARROW} \
         attributes, i.e. {SYSTEMS_PER_ATTR_BASELINE} per attribute plus a fixed {intercept}; the \
         intercept baseline is {SYSTEMS_INTERCEPT_BASELINE} (the never-run graph, 12 + 6a)",
        narrow.systems,
    );
    assert!(
        components <= COMPONENTS_PER_ATTR_BASELINE,
        "measured {components} registered components per attribute: components().len() is {} at \
         {NARROW} attributes and {} at {WIDE}; baseline is {COMPONENTS_PER_ATTR_BASELINE}",
        narrow.components,
        wide.components,
    );

    // Printed only: reflection counts track every registered type in the app, dev-deps included, so
    // they move without anything here changing. See the doc comment.
    println!(
        "B-1 AppTypeRegistry: {} reflected types at {NARROW} attributes, {} at {WIDE} \
         ({} per attribute, not asserted)",
        narrow.reflected,
        wide.reflected,
        slope(narrow.reflected, wide.reflected, "AppTypeRegistry len()"),
    );
}

/// The DAG resolver registers nothing per attribute: the periodic fires, the instant base writes, the
/// base-write scan and the walk are each one node for the whole app, and v1's per-attribute systems
/// and observers sit behind the early return in `init_effect_attribute`.
///
/// Counted over the whole app rather than over `FixedUpdate`, and exact rather than one-sided: the
/// slope-only read stayed green for a node parked in `PreUpdate` and for a fourth once-per-app node.
/// The at-rest counters cannot see the per-attribute state the collapsed nodes install on first use,
/// so the same app is read again after it has actually ticked.
#[test]
fn dag_registration_cost_per_attribute_is_pinned() {
    let narrow = measure_dag(NARROW);
    let wide = measure_dag(WIDE);

    pin_dag(
        "scheduled node",
        narrow.systems,
        wide.systems,
        DAG_SYSTEMS_BASELINE,
    );
    pin_dag(
        "observer",
        narrow.observers,
        wide.observers,
        DAG_OBSERVERS_BASELINE,
    );

    let narrow = measure_dag_tick(NARROW);
    let wide = measure_dag_tick(WIDE);

    pin_dag_per_attribute(
        "resource",
        narrow.resources,
        wide.resources,
        DAG_TICK_RESOURCES_BASELINE,
        DAG_TICK_RESOURCES_PER_ATTRIBUTE,
    );
    pin_dag_per_attribute(
        "component",
        narrow.components,
        wide.components,
        DAG_TICK_COMPONENTS_BASELINE,
        DAG_TICK_COMPONENTS_PER_ATTRIBUTE,
    );
}

/// Pins a counter a tick leaves behind: the exact count at [`NARROW`] attributes plus an exact
/// per-attribute delta. The DAG counters at rest pin a delta of zero; these do not, because the
/// collapsed nodes install state per attribute the first time they run.
fn pin_dag_per_attribute(
    counter: &str,
    narrow: usize,
    wide: usize,
    baseline: usize,
    per_attribute: usize,
) {
    let delta = wide as isize - narrow as isize;
    let pinned = (per_attribute * STEP) as isize;
    let where_to_look = "the per-attribute part of this counter is the message resource \
         `init_effect_attribute` adds (vitality_effects/src/lib.rs:142) and the query states the \
         collapsed nodes install on first use: the base-write scan \
         (base_write_scan, vitality_core/src/registry.rs:473), the instant apply \
         (apply_queued_modifiers_of, vitality_effects/src/modifier.rs:323) and the periodic apply \
         (apply_periodic_effects_of, vitality_effects/src/periodic.rs:52); the lists those nodes \
         walk are built in init_effect_attribute's DAG branch \
         (vitality_effects/src/lib.rs:144-154)";
    assert_eq!(
        delta, pinned,
        "the ticked DAG {counter} count moved by {delta} between {NARROW} and {WIDE} attributes \
         ({narrow} -> {wide}): the per-attribute delta is pinned at {per_attribute}, i.e. {pinned} \
         over the {STEP} attributes between the two builds. {where_to_look}.",
    );
    assert_eq!(
        narrow, baseline,
        "the fixed ticked DAG {counter} count moved: a DAG app at {NARROW} attributes carries \
         {narrow} after one tick, the baseline is {baseline}. The per-attribute delta \
         ({per_attribute}) is pinned separately. {where_to_look}.",
    );
}

/// Pins one DAG counter on two numbers: the exact count at [`NARROW`] attributes and a delta of
/// exactly zero to [`WIDE`]. Two messages, because a red pin has to say which half moved.
fn pin_dag(counter: &str, narrow: usize, wide: usize, baseline: usize) {
    let delta = wide as isize - narrow as isize;
    assert_eq!(
        delta, 0,
        "the DAG {counter} count grew by {delta} between {NARROW} and {WIDE} attributes ({narrow} \
         -> {wide}, {STEP} attributes apart): the per-attribute delta is pinned at 0, and the \
         fixed half is pinned separately at {baseline}.",
    );
    assert_eq!(
        narrow, baseline,
        "the fixed DAG {counter} count moved: a DAG app at {NARROW} attributes carries {narrow}, \
         the baseline is {baseline}. The per-attribute delta (0) is pinned separately.",
    );
}

/// The two at-rest structural counters a DAG app is pinned on.
#[derive(Clone, Copy, Debug)]
struct DagCounters {
    /// Every scheduled node the app owns, summed over every schedule in it.
    systems: usize,
    /// Observer entities, which no schedule count can see.
    observers: usize,
}

/// What one attribute costs a DAG app that has actually run: the resources and components left over
/// after one fixed tick.
#[derive(Clone, Copy, Debug)]
struct DagTickCounters {
    /// Resources at rest, before the tick.
    resting: usize,
    /// Resources after the tick, where the collapsed nodes' per-attribute state shows up.
    resources: usize,
    components: usize,
}

/// [`measure_dag`]'s app, ticked once. `TimeUpdateStrategy::FixedTimesteps(1)` makes the second
/// `update` exactly one fixed tick; the first primes bevy's clock, whose delta is zero, so it runs
/// none.
fn measure_dag_tick(attributes: usize) -> DagTickCounters {
    let mut app = dag_app(attributes);
    // counted after the insert, so only the tick's own state can widen the gap the guard reads
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    let resting = app.world().iter_resources().count();
    app.update();
    app.update();
    let counters = DagTickCounters {
        resting,
        resources: app.world().iter_resources().count(),
        components: app.world().components().len(),
    };
    assert!(
        counters.resources >= counters.resting + attributes,
        "the ticked DAG app holds {} resources where it held {} at rest over {attributes} \
         attributes: the collapsed nodes install first-use state for every one of them, so less than \
         one new resource per attribute means the fixed schedule never ran and this counter measures \
         nothing",
        counters.resources,
        counters.resting,
    );
    println!("B-1 dag-tick a={attributes}: {counters:?}");
    counters
}

/// The same plugin set [`build_app_with`] uses, so the fixed half holds the once-per-app nodes
/// `EffectsPlugin` puts in the DAG branch. Never ticked, so `systems_len()` reads the graph.
fn measure_dag(attributes: usize) -> DagCounters {
    let mut app = dag_app(attributes);
    let systems = app
        .world()
        .get_resource::<Schedules>()
        .expect("the harness app has no schedules to count systems in")
        .iter()
        .map(|(_, schedule)| schedule.systems_len())
        .sum();
    let mut observers = app.world_mut().query::<&Observer>();
    let counters = DagCounters {
        systems,
        observers: observers.iter(app.world()).count(),
    };
    println!("B-1 dag a={attributes}: {counters:?}");
    counters
}
