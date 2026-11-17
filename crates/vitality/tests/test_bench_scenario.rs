//! Pins what each `scenario/*` workload does to a small world, so a scenario id means its workload.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/scenario.rs"]
mod scenario;

use bevy::prelude::*;
use common::ATTRIBUTE_POOL;
use scenario::{Workload, build, build_in};
use vitality::effect::ApplyEffectEvent;

const ACTORS: usize = 40;

fn bases(world: &mut scenario::ScenarioWorld, attr: usize) -> Vec<f64> {
    let actors = world.actors.clone();
    actors
        .iter()
        .map(|&actor| ATTRIBUTE_POOL[attr].base(world.app.world(), actor).unwrap())
        .collect()
}

fn moved(before: &[f64], after: &[f64]) -> usize {
    before.iter().zip(after).filter(|(a, b)| a != b).count()
}

// README's Performance numbers claim exactly these shares: steady moves 10 % of the actors, storm all
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

/// `World::trigger` runs its observers inline, so a `drive` that triggered instead of queueing has
/// already paid for the tag test, the condition eval and the stacking decision before the sample starts.
/// Nothing else builds this harness: an apply workload that applies nothing would still run a green tick.
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
        let mut world = build_in(workload, ACTORS);
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
