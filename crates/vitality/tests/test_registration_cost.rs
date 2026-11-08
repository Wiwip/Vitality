//! B-1: what one registered attribute costs the app, in structural counts rather than durations, so
//! the gate can pin it without a stopwatch. Measured at this tip (`2fdef8d`, bevy 0.20.1) with the
//! shape below: `FixedUpdate` `systems_len()` is **20 at 2 attributes and 140 at 22 → 6 scheduled
//! nodes per attribute**, and `world.components().len()` is **140 at 2 and 340 at 22 → 10 components
//! per attribute**. Both baselines are exactly those measured slopes — no headroom, so the next change
//! to `init_core_attribute` moves a number here rather than a guess. The slope matches the 6 nodes per
//! attribute `AUTO-REPORT-bench-2026-11-07.md` §1 quotes for the floor; that run's intercept sits 2
//! nodes higher (22 at two attributes, 20 here), and this file chases neither — the per-attribute cost
//! is the slope, and both runs agree on it.
//!
//! The shape is 1 actor, 0 effects and 0 modifiers on purpose: this prices *registration*, and every
//! per-actor or per-effect component would be charged into the per-attribute slope. Both widths come
//! from the default 24-deep [`common::ATTRIBUTE_POOL`], so no new attribute types are declared here.
//!
//! The third §2G counter — `AppTypeRegistry` registrations, 1152 at 2 and 1212 at 22 (3 per
//! attribute) — is printed, never asserted: reflection entries come from every registered type in the
//! app, `MinimalPlugins`/`AssetPlugin` included, so they move with unrelated dev-dependencies and a
//! bound on them would go red for reasons this file has nothing to do with.

#[path = "../benches/support/common.rs"]
mod common;

use crate::common::{ATTRIBUTE_POOL, WorldSpec, build_app_with};
use bevy::prelude::*;

/// The two widths the slope is taken between. 22 is the widest the default pool can serve beside 2.
const NARROW: usize = 2;
const WIDE: usize = 22;

/// Attributes between the two builds, i.e. the divisor of every slope here.
const STEP: usize = WIDE - NARROW;

/// The measured slopes, so a regression is red rather than merely slower. Measured values are printed
/// in the assert messages too.
const SYSTEMS_PER_ATTR_BASELINE: usize = 6;
const COMPONENTS_PER_ATTR_BASELINE: usize = 10;

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
