//! Pins what each `scenario/*` workload does to a small world, so a scenario id means its workload.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/scenario.rs"]
mod scenario;

use bevy::prelude::*;
use common::{ATTRIBUTE_POOL, Attr1, Attr8};
use scenario::{Workload, build};
use vitality::effect::modifier::Capture;
use vitality::effect::{AppliedEffects, ApplyEffectEvent};
use vitality::prelude::*;

const ACTORS: usize = 40;

fn bases(world: &mut scenario::ScenarioWorld, attr: usize) -> Vec<f64> {
    let actors = world.actors.clone();
    actors
        .iter()
        .map(|&actor| ATTRIBUTE_POOL[attr].base(world.app.world(), actor).unwrap())
        .collect()
}

fn currents(world: &mut scenario::ScenarioWorld, attr: usize) -> Vec<f64> {
    let actors = world.actors.clone();
    actors
        .iter()
        .map(|&actor| {
            ATTRIBUTE_POOL[attr]
                .current(world.app.world(), actor)
                .unwrap()
        })
        .collect()
}

fn moved(before: &[f64], after: &[f64]) -> usize {
    before.iter().zip(after).filter(|(a, b)| a != b).count()
}

/// Effect entities on one actor, including the three every actor is built with.
fn applied(world: &mut scenario::ScenarioWorld, actor: Entity) -> usize {
    world
        .app
        .world()
        .get::<AppliedEffects>(actor)
        .map(|applied| applied.iter().count())
        .unwrap_or(0)
}

#[derive(Resource, Default)]
struct Blocks(Vec<Entity>);

#[test]
fn write_workloads_touch_the_share_their_id_names() {
    for (workload, share) in [
        (Workload::Idle, 0),
        (Workload::Steady, ACTORS / 10),
        (Workload::Storm, ACTORS),
        (Workload::NoOpWrite, 0),
        (Workload::Damage, ACTORS / 10),
    ] {
        let mut world = build(workload, ACTORS);
        let before = bases(&mut world, 0);
        world.drive();
        world.tick();
        let after = bases(&mut world, 0);
        assert_eq!(moved(&before, &after), share, "{workload:?}");
    }
}

#[test]
fn the_aura_follows_its_caster() {
    let mut world = build(Workload::Aura, ACTORS);
    let target = world.actors[1];
    let before = world
        .app
        .world()
        .get::<Attr1>(target)
        .unwrap()
        .current_value();
    world.drive();
    world.tick();
    world.tick();
    let after = world
        .app
        .world()
        .get::<Attr1>(target)
        .unwrap()
        .current_value();
    assert_ne!(
        before, after,
        "a caster write must reach its aura's targets"
    );
}

#[test]
fn churn_workloads_keep_their_population_bounded() {
    let mut churn = build(Workload::BuffChurn, ACTORS);
    let baseline = churn.effect_count();
    for _ in 0..20 {
        churn.drive();
        churn.tick();
    }
    let extra = churn.effect_count() - baseline;
    assert!(
        extra > 0 && extra <= 4 * ACTORS / 10,
        "buffs live three ticks, so about three ticks' worth stay up; saw {extra}"
    );

    let mut spawn = build(Workload::Spawn, ACTORS);
    for _ in 0..5 {
        spawn.drive();
        spawn.tick();
    }
    assert_eq!(spawn.actor_count(), ACTORS);

    let mut combat = build(Workload::Combat, ACTORS);
    let before: f64 = bases(&mut combat, 3).iter().sum();
    combat.drive();
    combat.tick();
    let after: f64 = bases(&mut combat, 3).iter().sum();
    assert_eq!(before - after, (ACTORS / 5) as f64);
}

/// Every apply the timed window is read for has to happen inside it: `World::trigger` runs its
/// observers inline, so a `drive` that triggered instead of queueing has already paid for the tag
/// test, the condition eval and the stacking decision before the sample starts.
#[test]
fn drive_queues_the_applies_and_tick_dispatches_them() {
    for (workload, applies) in [
        (Workload::BuffChurn, ACTORS / 10),
        (Workload::Combat, ACTORS / 5),
        (Workload::StackAdd, ACTORS / 10),
        (Workload::StackRefresh, ACTORS / 10),
        (Workload::TagRequired, ACTORS / 5),
        (Workload::Immunity, ACTORS / 5),
        (Workload::Snapshot, ACTORS / 10),
    ] {
        let mut world = build(workload, ACTORS);
        // on the world: `App::add_observer` waits for the next `First`, which this tick never runs
        world
            .app
            .world_mut()
            .insert_resource(Dispatched(Vec::new()));
        world.app.world_mut().add_observer(
            |ev: On<ApplyEffectEvent>, mut dispatched: ResMut<Dispatched>| {
                dispatched.0.push(ev.entity);
            },
        );
        world.drive();
        let queued = world.app.world().resource::<Dispatched>().0.len();
        assert_eq!(queued, 0, "{workload:?}: drive must not dispatch an apply");
        world.tick();
        let dispatched = &world.app.world().resource::<Dispatched>().0;
        assert_eq!(
            dispatched.len(),
            applies,
            "{workload:?}: the timed window must contain every apply drive queued"
        );
    }
}

#[derive(Resource, Default)]
struct Dispatched(Vec<Entity>);

/// `stack_add` claims a stack per hit on one instance; `stack_refresh` claims one instance kept
/// alive by the refresh, which is a different claim and a different policy arm.
#[test]
fn stacking_workloads_stay_on_one_instance_and_move_what_they_claim() {
    let hits = ACTORS / 10;

    let mut add = build(Workload::StackAdd, ACTORS);
    let baseline = add.effect_count();
    let before = currents(&mut add, 4);
    // `hits` applies a tick, round-robin, so this many ticks visit every actor once and the first
    // window twice.
    for _ in 0..ACTORS / hits + 1 {
        add.drive();
        add.tick();
    }
    let after = currents(&mut add, 4);
    assert_eq!(
        add.effect_count(),
        baseline + ACTORS,
        "a stacking apply reuses the instance already up, so one effect entity per actor"
    );
    let step = after[hits] - before[hits];
    assert!(step > 0.0, "a stack that moves no value is not a stack");
    for (index, (was, now)) in before.iter().zip(&after).enumerate() {
        let climbs = if index < hits { 2.0 } else { 1.0 };
        assert!(
            (now - was - step * climbs).abs() < 1e-3,
            "actor {index} climbed {} steps, want {climbs}",
            (now - was) / step
        );
    }

    let mut refresh = build(Workload::StackRefresh, ACTORS);
    let baseline = refresh.effect_count();
    let before = currents(&mut refresh, 5);
    // every actor is built from one actor definition, so the gap to an untouched actor is the buff
    let gap = before[0] - before[2 * hits];
    assert!(
        gap > 0.0,
        "the refresh workload has a buff on its window to refresh"
    );
    // three-tick buffs, so ten ticks is long enough for one nobody refreshes to fall off
    for _ in 0..10 {
        refresh.drive();
        refresh.tick();
    }
    let after = currents(&mut refresh, 5);
    assert_eq!(
        refresh.effect_count(),
        baseline,
        "the refresh keeps the instances it applied up and never spawns a second one"
    );
    for index in 0..hits {
        let actor = refresh.actors[index];
        assert_eq!(
            applied(&mut refresh, actor),
            4,
            "actor {index} is still carrying the one buff it was built with"
        );
    }
    let bystander = refresh.actors[2 * hits];
    assert_eq!(
        applied(&mut refresh, bystander),
        3,
        "and an actor outside the window never gained one"
    );
    assert_eq!(
        moved(&before, &after),
        0,
        "a refreshed buff is topped up every tick, so its contribution never blinks"
    );
    assert_eq!(
        after[0] - after[2 * hits],
        gap,
        "and the window is still carrying it past the buff's own duration"
    );
}

/// `tag_required` lands on the tagged half of the actors it visits, `immunity` on the untagged
/// half: one visit rule, the gate flipped.
#[test]
fn the_tag_rule_decides_which_visited_actors_take_the_effect() {
    let visited = ACTORS / 5;

    let mut gated = build(Workload::TagRequired, ACTORS);
    let before = bases(&mut gated, 6);
    gated.drive();
    gated.tick();
    let after = bases(&mut gated, 6);
    assert_eq!(
        moved(&before, &after),
        visited / 2,
        "half of the visited actors carry the tag the effect asks for"
    );
    for index in 0..visited {
        let took = after[index] != before[index];
        assert_eq!(
            took,
            index % 2 == 0,
            "actor {index} was visited: only the tagged (even) half can take a required-tag effect"
        );
    }

    let mut immune = build(Workload::Immunity, ACTORS);
    // on the world, not the app: `App::add_observer` waits for the next `First`, and the block
    // fires during the flush that opens this tick.
    immune.app.world_mut().insert_resource(Blocks::default());
    immune
        .app
        .world_mut()
        .add_observer(|ev: On<EffectBlocked>, mut blocks: ResMut<Blocks>| {
            blocks.0.push(ev.entity);
        });
    let before = bases(&mut immune, 7);
    immune.drive();
    immune.tick();
    let after = bases(&mut immune, 7);
    for index in 0..visited {
        let took = after[index] != before[index];
        assert_eq!(
            took,
            index % 2 == 1,
            "actor {index} was visited: the immune (even) half must be the half that takes nothing"
        );
    }
    assert_eq!(
        immune.app.world().resource::<Blocks>().0.len(),
        visited / 2,
        "every apply the immunity stopped says so"
    );
}

/// `snapshot` claims the value its modifier reads is taken at apply time: the buff is worth what
/// the expression read on the tick it landed, and the capture is the library's own.
#[test]
fn the_snapshot_buff_keeps_the_value_it_captured() {
    let hits = ACTORS / 10;
    let mut world = build(Workload::Snapshot, ACTORS);
    let target = world.actors[0];
    let captured = ATTRIBUTE_POOL[1]
        .current(world.app.world(), target)
        .expect("target carries the attribute the buff reads");
    let before = currents(&mut world, 8);
    world.drive();
    world.tick();
    let after = currents(&mut world, 8);
    assert_eq!(
        moved(&before, &after),
        hits,
        "one fresh buff for each actor the workload picked"
    );
    let gained = ATTRIBUTE_POOL[8]
        .current(world.app.world(), target)
        .unwrap()
        - before[0];
    assert!(
        (gained - captured).abs() < 1e-3,
        "the buff is worth what its expression read at apply time: {gained}, want {captured}"
    );

    let captures = modifier_captures(&mut world);
    assert!(
        !captures.is_empty() && captures.iter().all(|capture| *capture == Capture::Snapshot),
        "the id says snapshot, so every modifier this workload applies has to capture: {captures:?}"
    );

    ATTRIBUTE_POOL[1].set_base(world.app.world_mut(), target, captured + 100.0);
    for _ in 0..2 {
        world.tick();
    }
    assert_eq!(
        ATTRIBUTE_POOL[8].current(world.app.world(), target),
        Some(after[0]),
        "a captured modifier does not follow the attribute it read"
    );
}

/// Capture mode of every `Attr8` modifier in the world, which is only the snapshot workload's.
fn modifier_captures(world: &mut scenario::ScenarioWorld) -> Vec<Capture> {
    let world = world.app.world_mut();
    let mut query = world.query::<&AttributeModifier<Attr8>>();
    query.iter(world).map(|modifier| modifier.capture).collect()
}
