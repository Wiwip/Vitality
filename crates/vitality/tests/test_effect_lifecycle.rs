mod gameplay;

use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

#[test]
fn instant_effect_changes_the_base_value() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let heal = effect(
        &mut app,
        EffectBuilder::instant()
            .modify::<Health>(-30.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectApplied>(&mut app);

    apply(&mut app, hero, hero, &heal);
    ticks(&mut app, 1);

    assert_eq!(base::<Health>(&app, hero), 70.0);
    assert_eq!(current::<Health>(&app, hero), 70.0);
    let applied = recorded::<EffectApplied>(&app);
    assert_eq!(applied.len(), 1);
    assert_eq!(
        applied[0].effect, None,
        "an instant effect spawns no entity"
    );
}

#[test]
fn permanent_buff_applies_and_removal_reverts_it() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    assert_eq!(current::<Power>(&app, hero), 15.0);
    assert_eq!(
        base::<Power>(&app, hero),
        10.0,
        "a buff never touches the base"
    );

    let buff_handle = buff.clone();
    let removed = with_effects(&mut app, move |effects| {
        effects.remove_by_def(hero, &buff_handle)
    });
    assert_eq!(removed, 1);
    ticks(&mut app, 1);

    assert_eq!(
        current::<Power>(&app, hero),
        10.0,
        "removing the buff must revert it"
    );
    let events = recorded::<EffectRemoved>(&app);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].reason, EffectRemovalReason::Removed);
}

#[test]
fn temporary_buff_expires_and_reverts() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::for_seconds(0.25)
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    assert_eq!(current::<Power>(&app, hero), 15.0);

    ticks(&mut app, ticks_for(0.25) - 3);
    assert_eq!(current::<Power>(&app, hero), 15.0, "expired early");

    ticks(&mut app, 5);
    assert_eq!(
        current::<Power>(&app, hero),
        10.0,
        "the expired buff still applies"
    );
    let events = recorded::<EffectRemoved>(&app);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].reason, EffectRemovalReason::Expired);
    let left = with_effects(&mut app, move |effects| effects.on(hero).len());
    assert_eq!(left, 0);
}

#[test]
fn periodic_effect_fires_once_per_interval() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let regen = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.25)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, hero, hero, &regen);
    ticks(&mut app, ticks_for(1.0) + 1);

    assert_eq!(
        base::<Health>(&app, hero),
        4.0,
        "four intervals in one second"
    );
    assert_eq!(current::<Health>(&app, hero), 4.0);
}

#[test]
fn periodic_effect_with_a_duration_stops_firing() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let dot = effect(
        &mut app,
        EffectBuilder::every_second_for_duration(0.25, 0.5)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, hero, hero, &dot);
    ticks(&mut app, ticks_for(2.0));

    assert_eq!(
        base::<Health>(&app, hero),
        2.0,
        "fires at 0.25 and 0.5, then expires"
    );
}

#[test]
fn removal_by_source_and_by_tag_only_hits_matching_effects() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let enemy = spawn(&mut app, hero_def());
    let burn = effect(
        &mut app,
        EffectBuilder::permanent()
            .with_tag::<Burning>()
            .modify::<Armor>(-1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let ward = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, enemy, hero, &burn);
    apply(&mut app, hero, hero, &ward);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), 9.0);
    assert!(
        has::<Burning>(&app, hero),
        "the burn grants its tag to the target"
    );

    let removed = with_effects(&mut app, move |effects| {
        effects.remove_from_source(hero, hero)
    });
    assert_eq!(removed, 1);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), -1.0);

    let removed = with_effects(&mut app, move |effects| {
        effects.remove_with_tag::<Burning>(hero)
    });
    assert_eq!(removed, 1);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), 0.0);
    assert!(
        !has::<Burning>(&app, hero),
        "the tag goes with its last effect"
    );
}

#[test]
fn an_effect_that_loses_its_source_reports_despawned() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let enemy = spawn(&mut app, hero_def());
    let curse = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, enemy, hero, &curse);
    ticks(&mut app, 1);
    assert_eq!(current::<Power>(&app, hero), 15.0);
    let curse_entity = with_effects(&mut app, move |effects| effects.on(hero))[0];

    // EffectSource is a linked_spawn relationship, so the effect entity dies with its caster
    app.world_mut().despawn(enemy);
    ticks(&mut app, 1);

    let events = recorded::<EffectRemoved>(&app);
    assert_eq!(events.len(), 1, "the despawn still reports a removal");
    assert_eq!(events[0].reason, EffectRemovalReason::Despawned);
    assert_eq!(events[0].entity, hero, "the event names the target actor");
    assert_eq!(events[0].effect, curse_entity);
    assert_eq!(current::<Power>(&app, hero), 10.0, "the buff dies with it");
}

/// Period of the ticker: 0.25 s is 16 of the harness' 1/64 s ticks.
const PERIOD: f32 = 0.25;

/// Twice the period, so the duration runs out on the tick the second fire is due.
const DURATION: f32 = 2.0 * PERIOD;

/// Fires a dot has delivered by the tick its duration runs out, that tick's fire included.
const FIRES_BY_EXPIRY: f32 = 2.0;

#[test]
fn a_fire_due_on_the_expiry_tick_beats_the_expiry() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let dot = effect(
        &mut app,
        EffectBuilder::every_second_for_duration(PERIOD, DURATION)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    // the whole point of the shape: first fire on tick 16, expiry on tick 32 = fire number two
    let expiry_tick = ticks_for(DURATION);
    assert_eq!(expiry_tick, 2 * ticks_for(PERIOD));

    apply(&mut app, hero, hero, &dot);
    ticks(&mut app, expiry_tick);

    assert_eq!(
        base::<Health>(&app, hero),
        FIRES_BY_EXPIRY,
        "the tick that expires the effect also owes it a fire, and the expiry must not swallow it",
    );
    let events = recorded::<EffectRemoved>(&app);
    assert_eq!(events.len(), 1, "the expiry is not postponed by the fire");
    assert_eq!(events[0].reason, EffectRemovalReason::Expired);

    ticks(&mut app, ticks_for(PERIOD));
    assert_eq!(
        base::<Health>(&app, hero),
        FIRES_BY_EXPIRY,
        "a dot that expired fires no more"
    );
    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(hero).len()),
        0
    );

    // what makes the number above hold is the schedule edge, not the applier: the collapsed periodic
    // node has to write its fire before tick_effect_durations despawns the effect. v1 declares no
    // such edge and leaves the tie-break open (D-53), so only the behaviour above is shared.
    if mode() == vitality::ResolverMode::Dag {
        // a schedule moves its systems out of its graph the first time it runs, so the edges are read
        // off a fresh app of the same shape rather than this one
        let built = gameplay::app();
        let schedule = built
            .world()
            .resource::<bevy::ecs::schedule::Schedules>()
            .get(bevy::app::FixedUpdate)
            .expect("the harness app has no FixedUpdate schedule");
        assert!(
            ordered_before(
                schedule,
                "apply_all_periodic_effects",
                "tick_effect_durations"
            ),
            "apply_all_periodic_effects is no longer ordered ahead of tick_effect_durations in \
             FixedUpdate, which is what delivers the fire above; the edge lives in \
             vitality_effects::EffectsPlugin's DAG branch.",
        );
    }
}

/// True if any `FixedUpdate` node whose name contains `from` has to run before one containing `to`,
/// read off the schedule's declared dependencies rather than off an execution order.
fn ordered_before(schedule: &bevy::ecs::schedule::Schedule, from: &str, to: &str) -> bool {
    let graph = schedule.graph();
    let dag = graph.dependency().graph();
    let named = |needle: &str| {
        dag.nodes()
            .filter(|node| graph.get_node_name(node).contains(needle))
            .collect::<Vec<_>>()
    };
    let targets = named(to);
    let mut pending = named(from);
    let mut seen: Vec<_> = Vec::new();
    while let Some(node) = pending.pop() {
        if targets.contains(&node) {
            return true;
        }
        if seen.contains(&node) {
            continue;
        }
        seen.push(node);
        pending.extend(dag.neighbors(node));
    }
    false
}
