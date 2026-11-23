mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::prelude::*;
use gameplay::*;
use vitality::effect::conditions::HasComponent;
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

    let buff_handle = buff;
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

/// the bare `Entity` of `Effects::remove` is the one removal argument the runtime cannot trust
#[test]
fn removing_an_entity_that_is_not_an_effect_spares_it() {
    let logs = Logs::scoped();

    let (app, hero) = logs.run(|| {
        let mut app = app();
        let hero = spawn(&mut app, hero_def());
        let buff = effect(
            &mut app,
            EffectBuilder::permanent()
                .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        apply(&mut app, hero, hero, &buff);
        ticks(&mut app, 1);

        with_effects(&mut app, move |effects| effects.remove(hero));
        ticks(&mut app, 2);
        (app, hero)
    });

    assert!(
        has::<Health>(&app, hero),
        "handing the actor to Effects::remove instead of an effect entity despawned it"
    );
    let warns = logs.warns();
    assert!(
        warns.contains("not an effect") && warns.contains(&hero.to_string()),
        "the dropped removal has to name the entity and why: {warns}"
    );
}

/// `Effects::remove_stacks` takes the same untrusted `Entity`
#[test]
fn removing_stacks_from_an_entity_that_is_not_an_effect_spares_it() {
    let logs = Logs::scoped();

    let (app, hero) = logs.run(|| {
        let mut app = app();
        let hero = spawn(&mut app, hero_def());
        let buff = effect(
            &mut app,
            EffectBuilder::permanent()
                .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        apply(&mut app, hero, hero, &buff);
        ticks(&mut app, 1);

        with_effects(&mut app, move |effects| effects.remove_stacks(hero, 1));
        ticks(&mut app, 2);
        (app, hero)
    });

    assert_eq!(
        current::<Power>(&app, hero),
        15.0,
        "handing the actor to Effects::remove_stacks instead of an effect entity took its buff away"
    );
    let warns = logs.warns();
    assert!(
        warns.contains("Stack removal dropped") && warns.contains(&hero.to_string()),
        "the dropped stack removal has to name the entity and why: {warns}"
    );
}

/// a target event an `add_actor_trigger` watches, like an on-hit riposte
#[derive(EntityEvent, Clone)]
struct LocalStruck {
    #[event_target]
    pub target: Entity,
}

#[derive(Resource, Default)]
struct Hits(pub u32);

#[test]
fn trigger_of_a_removed_effect_never_fires() {
    let mut app = app();
    app.insert_resource(Hits::default());
    let hero = spawn(&mut app, hero_def());
    let spawned = app.world().entities().count_spawned();
    let riposte = effect(
        &mut app,
        EffectBuilder::permanent()
            .add_actor_trigger(|_: On<LocalStruck>, mut hits: ResMut<Hits>| hits.0 += 1)
            .build(),
    );

    apply(&mut app, hero, hero, &riposte);
    ticks(&mut app, 1);

    app.world_mut().trigger(LocalStruck { target: hero });
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<Hits>().0,
        1,
        "applied, the trigger answers the event on its target"
    );

    let effect_entity = with_effects(&mut app, move |effects| effects.on(hero))[0];
    with_effects(&mut app, move |effects| effects.remove(effect_entity));
    ticks(&mut app, 1);

    app.world_mut().trigger(LocalStruck { target: hero });
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<Hits>().0,
        1,
        "the removed effect's trigger entity has to go with the effect"
    );
    assert_eq!(
        app.world().entities().count_spawned(),
        spawned,
        "removal has to take the effect's trigger entity along"
    );
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

/// Half the harness' 1/64 s tick, so two intervals fit in one tick and both are exact in binary.
const SUB_TICK_PERIOD: f32 = 1.0 / 128.0;
const TICK_COUNT: usize = 8;

#[test]
fn periodic_effect_faster_than_a_tick_fires_every_interval() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let twitch = effect(
        &mut app,
        EffectBuilder::every_second_permanently(SUB_TICK_PERIOD)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, hero, hero, &twitch);
    ticks(&mut app, TICK_COUNT);

    assert_eq!(
        base::<Health>(&app, hero),
        2.0 * TICK_COUNT as f32,
        "an interval of {} s is two fires per {} s tick, so each of the {} ticks owes both",
        SUB_TICK_PERIOD,
        STEP.as_secs_f32(),
        TICK_COUNT,
    );
}

/// Seven sub-tick intervals, so the duration runs out between the seventh and the eighth fire.
const SUB_TICK_DURATION: f32 = 7.0 * SUB_TICK_PERIOD;

/// Fires owed by that duration, the one landing exactly on the expiry instant included.
const SUB_TICK_FIRES_BY_EXPIRY: f32 = 7.0;

#[test]
fn a_sub_tick_periodic_effect_fires_nothing_after_its_expiry() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let twitch = effect(
        &mut app,
        EffectBuilder::every_second_for_duration(SUB_TICK_PERIOD, SUB_TICK_DURATION)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, hero, hero, &twitch);
    ticks(&mut app, ticks_for(SUB_TICK_DURATION) + 2);

    assert_eq!(
        base::<Health>(&app, hero),
        SUB_TICK_FIRES_BY_EXPIRY,
        "the {} s tick carries two intervals, so the tick the {} s duration runs out on owes the \
         fire at the expiry and not the one that falls after it",
        STEP.as_secs_f32(),
        SUB_TICK_DURATION,
    );
}

#[test]
fn a_zero_interval_periodic_effect_fires_once_per_tick() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let drip = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.0)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, hero, hero, &drip);
    ticks(&mut app, 3);

    assert_eq!(
        base::<Health>(&app, hero),
        3.0,
        "a tick holds no whole interval of 0 s, so a zero interval still owes exactly one fire a tick"
    );
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

#[test]
fn a_starting_effect_reads_the_attributes_the_builder_gave() {
    let mut app = app();
    let starting = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );

    let hero = spawn(&mut app, hero_def().with_effect(&starting));
    ticks(&mut app, 1);

    assert_eq!(
        current::<Health>(&app, hero),
        110.0,
        "a starting effect reads the Power the builder put on the actor",
    );
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
}

/// what the fire above depends on, read off the graph: an executed schedule drops its declared edges
#[test]
fn the_periodic_fire_is_ordered_ahead_of_the_duration_walk() {
    assert!(
        ordered_before("apply_all_periodic_effects", "tick_effect_durations"),
        "apply_all_periodic_effects is no longer ordered ahead of tick_effect_durations in \
         FixedUpdate, which is what lets `a_fire_due_on_the_expiry_tick_beats_the_expiry` land \
         its last fire; the edge lives in vitality_effects::EffectsPlugin's build.",
    );
}

#[test]
fn a_periodic_effect_holds_its_first_fire_until_the_interval_elapses() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let regen = effect(
        &mut app,
        EffectBuilder::every_second_permanently(PERIOD)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, hero, hero, &regen);
    ticks(&mut app, ticks_for(PERIOD) - 1);

    assert_eq!(
        base::<Health>(&app, hero),
        0.0,
        "a ticker fired within {} ticks of an interval of {}, so the interval does not gate the fire",
        ticks_for(PERIOD) - 1,
        PERIOD,
    );

    ticks(&mut app, 1);
    assert_eq!(
        base::<Health>(&app, hero),
        1.0,
        "the quiet window above is a waiting ticker, not a dead one: the tick its interval elapses on \
         has to fire",
    );
}

#[test]
fn a_gated_off_periodic_effect_pauses_its_duration() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let dot = effect(
        &mut app,
        EffectBuilder::every_second_for_duration(PERIOD, DURATION)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .active_while(HasComponent::<Immune>::target())
            .build(),
    );

    apply(&mut app, hero, hero, &dot);
    ticks(&mut app, ticks_for(DURATION) + ticks_for(PERIOD));

    assert_eq!(
        base::<Health>(&app, hero),
        0.0,
        "a gated-off effect fired something"
    );
    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(hero).len()),
        1,
        "the gated-off window outlasted the duration, so the effect expired without firing"
    );

    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    ticks(&mut app, ticks_for(PERIOD) + 1);

    assert_eq!(
        base::<Health>(&app, hero),
        1.0,
        "the flag is on, so the ticker that waited out the gate owes its fire"
    );
}

#[test]
fn a_gate_closing_on_the_tick_the_duration_runs_out_freezes_it() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::for_seconds(2.0 * STEP.as_secs_f32())
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .active_while(HasComponent::<Immune>::target())
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);

    with_vitality(&mut app, move |v| v.revoke_tag::<Immune>(hero));
    ticks(&mut app, 1);

    assert!(
        recorded::<EffectRemoved>(&app).is_empty(),
        "the gate closed on the second of the two steps the duration owed, so the effect had to \
         freeze instead of being reaped `Expired`"
    );
    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(hero).len()),
        1,
        "a gated-off effect is held until the gate opens again"
    );
}

/// the freeze above's edge: with it gone the executor still runs the verdict first, so no behavioural cell reddens
#[test]
fn the_condition_verdict_is_ordered_ahead_of_the_duration_walk() {
    assert!(
        ordered_before("evaluate_effect_conditions", "tick_effect_durations"),
        "evaluate_effect_conditions is no longer ordered ahead of tick_effect_durations in \
         FixedUpdate, which is what lets `a_gate_closing_on_the_tick_the_duration_runs_out_freezes_it` \
         freeze instead of expiring; the edge is `.before(tick_effect_tickers)`, and the duration \
         walk follows it through the chain.",
    );
}

/// the gate closes one tick after a fire and opens again mid-interval, with the ticker frozen part way
#[test]
fn a_reactivated_periodic_effect_waits_for_its_interval_again() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let dot = effect(
        &mut app,
        EffectBuilder::every_second_permanently(PERIOD)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .active_while(HasComponent::<Immune>::target())
            .build(),
    );

    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    apply(&mut app, hero, hero, &dot);
    ticks(&mut app, ticks_for(PERIOD));
    assert_eq!(
        base::<Health>(&app, hero),
        1.0,
        "the gate is open, so the first interval is owed"
    );

    with_vitality(&mut app, move |v| v.revoke_tag::<Immune>(hero));
    ticks(&mut app, 4);
    let owed = base::<Health>(&app, hero);
    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    ticks(&mut app, 1);
    assert_eq!(
        base::<Health>(&app, hero),
        owed,
        "the re-activation tick owes a fire its ticker never counted"
    );

    ticks(&mut app, ticks_for(PERIOD) - 4);
    assert_eq!(
        base::<Health>(&app, hero),
        owed,
        "a paused window is no interval"
    );

    ticks(&mut app, 6);
    assert_eq!(
        base::<Health>(&app, hero),
        owed + 1.0,
        "the open ticker owes its next fire once the interval is paid"
    );
}

/// read off the schedule's declared dependencies, not off an execution order
fn ordered_before(from: &str, to: &str) -> bool {
    let built = app();
    let schedule = built
        .world()
        .resource::<bevy::ecs::schedule::Schedules>()
        .get(bevy::app::FixedUpdate)
        .expect("the harness app has no FixedUpdate schedule");
    let graph = schedule.graph();
    let dag = graph.dependency().graph();
    let named = |needle: &str| {
        dag.nodes()
            .filter(|node| graph.get_node_name(node).contains(needle))
            .collect::<Vec<_>>()
    };
    let targets = named(to);
    let mut pending = named(from);
    loop {
        let Some(node) = pending.pop() else {
            return false;
        };
        if targets.contains(&node) {
            return true;
        }
        pending.extend(dag.neighbors(node));
    }
}
