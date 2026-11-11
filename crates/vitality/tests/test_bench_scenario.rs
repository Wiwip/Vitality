//! Pins what each `scenario/*` workload does to a small world, so a scenario id means its workload.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/scenario.rs"]
mod scenario;

use common::{ATTRIBUTE_POOL, Attr1};
use scenario::{Workload, build};
use vitality::prelude::*;

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
