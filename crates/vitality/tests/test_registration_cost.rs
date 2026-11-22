//! What one registered attribute costs the app, in structural counts, not durations; one-sided bounds.

#[path = "../benches/support/common.rs"]
mod common;

use crate::common::{ATTRIBUTE_POOL, WorldSpec, build_app_with};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

// 22 is the widest the default pool can serve beside 2
const NARROW: usize = 2;
const WIDE: usize = 22;

const STEP: usize = WIDE - NARROW;

const SYSTEMS_PER_ATTR_BASELINE: usize = 0;
/// five at rest: the four ids `register` takes plus bevy's `Messages<ApplyAttributeModifierMessage<T>>`
const COMPONENTS_PER_ATTR_BASELINE: usize = 5;

/// a node added at plugin build time is a +1 here, where the slope above still reads 0
const SYSTEMS_INTERCEPT_BASELINE: usize = 10;

fn dag_app(attributes: usize) -> App {
    let mut app = App::new();
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

/// exact, both directions: a fourth once-per-app node `EffectsPlugin` would add is a +1 here
const DAG_SYSTEMS_BASELINE: usize = 43;

/// not a schedule node, so only this counter sees one; 13 since the task judge became two observers
const DAG_OBSERVERS_BASELINE: usize = 13;

/// an at-rest counter cannot see the first-use states; the fixed part is `PeriodicPass` plus `DefNames`
const DAG_TICK_RESOURCES_BASELINE: usize = 67;
const DAG_TICK_RESOURCES_PER_ATTRIBUTE: usize = 3;

/// exact, so a new per-attribute component is red here; 144 since the condition walk's filter names
/// `EffectConditional`, which registers it in every app that installs `ConditionPlugin`
const DAG_TICK_COMPONENTS_BASELINE: usize = 144;
const DAG_TICK_COMPONENTS_PER_ATTRIBUTE: usize = 7;

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

/// a negative delta means a broken counter, not a cheap attribute
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
        systems == SYSTEMS_PER_ATTR_BASELINE,
        "measured {systems} scheduled nodes per attribute: FixedUpdate systems_len() is {} at \
         {NARROW} attributes and {} at {WIDE}; baseline is {SYSTEMS_PER_ATTR_BASELINE} \
         (the collapsed nodes are once per app, so nothing here is per attribute)",
        narrow.systems,
        wide.systems,
    );
    // a node added at plugin build time is +1 at both widths, so the slope above still reads 0
    let intercept = narrow
        .systems
        .saturating_sub(SYSTEMS_PER_ATTR_BASELINE * NARROW);
    assert!(
        intercept <= SYSTEMS_INTERCEPT_BASELINE,
        "measured {intercept} fixed scheduled nodes: FixedUpdate systems_len() is {} at {NARROW} \
         attributes, i.e. {SYSTEMS_PER_ATTR_BASELINE} per attribute plus a fixed {intercept}; the \
         intercept baseline is {SYSTEMS_INTERCEPT_BASELINE} (the never-run graph, 10 + 0a)",
        narrow.systems,
    );
    assert!(
        components <= COMPONENTS_PER_ATTR_BASELINE,
        "measured {components} registered components per attribute: components().len() is {} at \
         {NARROW} attributes and {} at {WIDE}; baseline is {COMPONENTS_PER_ATTR_BASELINE}",
        narrow.components,
        wide.components,
    );

    // not asserted: reflection counts move with every registered type in the app, dev-deps included
    println!(
        "B-1 AppTypeRegistry: {} reflected types at {NARROW} attributes, {} at {WIDE} \
         ({} per attribute, not asserted)",
        narrow.reflected,
        wide.reflected,
        slope(narrow.reflected, wide.reflected, "AppTypeRegistry len()"),
    );
}

/// also counted after a tick, as a slope-only read missed a node parked in `PreUpdate`
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
        Some(DAG_TICK_RESOURCES_BASELINE),
        DAG_TICK_RESOURCES_PER_ATTRIBUTE,
    );
    // the count holds bevy's `TimeReceiver`, which needs std
    pin_dag_per_attribute(
        "component",
        narrow.components,
        wide.components,
        Some(DAG_TICK_COMPONENTS_BASELINE),
        DAG_TICK_COMPONENTS_PER_ATTRIBUTE,
    );
}

/// the delta here is not zero: the collapsed nodes install state per attribute on first use
fn pin_dag_per_attribute(
    counter: &str,
    narrow: usize,
    wide: usize,
    baseline: Option<usize>,
    per_attribute: usize,
) {
    let delta = wide as isize - narrow as isize;
    let pinned = (per_attribute * STEP) as isize;
    let where_to_look = "the per-attribute part of this counter is the message resource \
         `init_effect_attribute` adds (vitality_effects/src/lib.rs) and the query states the \
         collapsed nodes install on first use: the base-write scan \
         (base_write_scan, vitality_core/src/registry.rs), the instant apply \
         (apply_queued_modifiers_of, vitality_effects/src/modifier.rs) and, for an attribute that \
         has a fire this tick, the periodic apply (apply_periodic_effects_of, \
         vitality_effects/src/periodic.rs); the fixed part carries the tick table the periodic node \
         walks once per tick (PeriodicPass, vitality_effects/src/periodic.rs). The lists those nodes \
         walk are built in init_effect_attribute (vitality_effects/src/lib.rs)";
    assert_eq!(
        delta, pinned,
        "the ticked DAG {counter} count moved by {delta} between {NARROW} and {WIDE} attributes \
         ({narrow} -> {wide}): the per-attribute delta is pinned at {per_attribute}, i.e. {pinned} \
         over the {STEP} attributes between the two builds. {where_to_look}.",
    );
    if let Some(baseline) = baseline {
        assert_eq!(
            narrow, baseline,
            "the fixed ticked DAG {counter} count moved: a DAG app at {NARROW} attributes carries \
             {narrow} after one tick, the baseline is {baseline}. The per-attribute delta \
             ({per_attribute}) is pinned separately. {where_to_look}.",
        );
    }
}

/// two messages, because a red pin has to say which half moved
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

#[derive(Clone, Copy, Debug)]
struct DagCounters {
    systems: usize,
    observers: usize,
}

#[derive(Clone, Copy, Debug)]
struct DagTickCounters {
    resting: usize,
    resources: usize,
    components: usize,
}

/// `FixedTimesteps(1)` makes the second `update` one tick: the first primes bevy's zero-delta clock
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

/// never ticked, so `systems_len()` reads the graph rather than the compiled schedule
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

/// `ActorRead::get` (what `Attribute::at()` resolves to) hands back the attribute it is asked for
mod leaf_view_read {

    use crate::common::{
        ATTRIBUTE_POOL, HarnessWorld, WorldSpec, actor_entities, build_app_at, current_values, tick,
    };
    use bevy::prelude::*;
    use std::any::type_name;
    use std::collections::HashSet;
    use vitality::attributes::Attribute;
    use vitality::effect::EffectsRef;
    use vitality_core::view::ActorRead;

    /// through the trait, so the inherent `get` cannot shadow what the library calls
    fn read<T: Attribute<Property = f32>>(view: &EffectsRef<'_, '_>) -> f64 {
        ActorRead::get::<T>(view)
            .unwrap_or_else(|| panic!("{} unreadable through the view", type_name::<T>()))
            .current_value() as f64
    }

    /// one read per attribute, so the guard covers everything an actor carries, not just slot 0
    macro_rules! read_pool {
    ($view:expr, $( $name:ident ),+ $(,)?) => {
        vec![$( read::<crate::common::$name>($view) ),+]
    };
}

    #[test]
    fn the_view_hands_back_the_attribute_it_is_asked_for() {
        let spec = WorldSpec {
            attributes_per_actor: crate::common::POOL_LEN,
            actors: 8,
            ..WorldSpec::default()
        };
        let mut app = build_app_at(&spec, ATTRIBUTE_POOL);
        // build_app_at only runs Startup, so nothing has resolved and every current_value is still a base.
        tick(&mut app);
        let pool = app.world().resource::<HarnessWorld>().attrs();
        assert_eq!(
            pool.len(),
            crate::common::POOL_LEN,
            "the reads below cover {} of the {} attributes this world carries",
            crate::common::POOL_LEN,
            pool.len()
        );

        let mut views = app.world_mut().query::<EffectsRef<'static, 'static>>();
        for actor in actor_entities(app.world_mut()) {
            let expected = current_values(app.world(), pool, actor);
            let distinct: HashSet<u64> = expected.iter().map(|value| value.to_bits()).collect();
            assert_eq!(
                distinct.len(),
                expected.len(),
                "two pool attributes share a current value on {actor}, so a swapped hand-back could \
             compare equal"
            );
            let view = views
                .get(app.world(), actor)
                .unwrap_or_else(|err| panic!("no attribute view on {actor}: {err}"));
            assert_eq!(
                read_pool!(
                    &view, Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9,
                    Attr10, Attr11, Attr12, Attr13, Attr14, Attr15, Attr16, Attr17, Attr18, Attr19,
                    Attr20, Attr21, Attr22, Attr23
                ),
                expected,
                "one read per pool attribute on {actor} of shape {}",
                spec.shape_id()
            );
        }
    }
}
