mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::ability::tasks::{Tasks, wait_task};
use vitality::ability::{AbilityBuilder, AbilityDef, TargetData};
use vitality::prelude::*;

fn ability(app: &mut App, def: AbilityDef) -> Handle<AbilityDef> {
    app.world_mut()
        .resource_mut::<Assets<AbilityDef>>()
        .add(def)
}

fn caster_with(app: &mut App, abilities: &[&Handle<AbilityDef>]) -> Entity {
    let mut builder = hero_def();
    for handle in abilities {
        builder = builder.grant_ability(handle);
    }
    spawn(app, builder)
}

fn activate(app: &mut App, caster: Entity, handle: &Handle<AbilityDef>) {
    let handle = handle.clone();
    let granted = with_abilities(app, move |a| {
        a.try_activate(caster, &handle, TargetData::SelfCast)
    });
    assert!(granted, "the ability is not granted");
}

fn ability_entity(app: &mut App, caster: Entity, handle: &Handle<AbilityDef>) -> Entity {
    let handle = handle.clone();
    with_abilities(app, move |a| {
        a.get_ability_entity_by_handle(caster, &handle)
    })
    .unwrap()
}

fn failures(app: &App) -> Vec<ActivationError> {
    recorded::<AbilityActivationFailed>(app)
        .iter()
        .map(|failure| failure.reason)
        .collect()
}

fn record_all(app: &mut App) {
    record::<AbilityActivated>(app);
    record::<AbilityActivationFailed>(app);
    record::<AbilityEnded>(app);
}

#[test]
fn cost_is_paid_once_and_the_cooldown_gates_reuse() {
    let mut app = app();
    let bolt = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .with_cooldown(0.5)
            .build(),
    );
    let caster = caster_with(&mut app, &[&bolt]);
    record_all(&mut app);

    activate(&mut app, caster, &bolt);
    ticks(&mut app, 2);
    assert_eq!(base::<Mana>(&app, caster), 40.0);
    assert_eq!(recorded::<AbilityActivated>(&app).len(), 1);
    assert_eq!(
        recorded::<AbilityEnded>(&app).len(),
        1,
        "no tasks: it ends at once"
    );

    activate(&mut app, caster, &bolt);
    ticks(&mut app, 1);
    assert_eq!(base::<Mana>(&app, caster), 40.0);
    assert_eq!(failures(&app), vec![ActivationError::OnCooldown]);

    ticks(&mut app, ticks_for(0.5));
    activate(&mut app, caster, &bolt);
    ticks(&mut app, 2);
    assert_eq!(base::<Mana>(&app, caster), 30.0);
}

#[test]
fn every_declared_cost_is_paid() {
    let mut app = app();
    let blood_magic = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .with_cost::<Health>(5.0)
            .build(),
    );
    let caster = caster_with(&mut app, &[&blood_magic]);

    activate(&mut app, caster, &blood_magic);
    ticks(&mut app, 2);
    assert_eq!(base::<Mana>(&app, caster), 40.0);
    assert_eq!(base::<Health>(&app, caster), 95.0);
}

#[test]
fn an_unaffordable_ability_fails_with_a_reason() {
    let mut app = app();
    let nuke = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Mana>(500.0).build(),
    );
    let caster = caster_with(&mut app, &[&nuke]);
    record_all(&mut app);

    activate(&mut app, caster, &nuke);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::CannotAfford]);
    assert_eq!(base::<Mana>(&app, caster), 50.0);
}

#[test]
fn an_ability_without_a_cooldown_can_be_reused() {
    let mut app = app();
    let jab = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Mana>(1.0).build(),
    );
    let caster = caster_with(&mut app, &[&jab]);

    for _ in 0..3 {
        activate(&mut app, caster, &jab);
        ticks(&mut app, 2);
    }
    assert_eq!(base::<Mana>(&app, caster), 47.0);
}

#[test]
fn a_stunned_caster_cannot_activate() {
    let mut app = app();
    let jab = ability(
        &mut app,
        AbilityBuilder::new()
            .activation_blocked_by::<Stunned>()
            .build(),
    );
    let caster = caster_with(&mut app, &[&jab]);
    record_all(&mut app);

    with_vitality(&mut app, move |v| v.grant_tag::<Stunned>(caster));
    activate(&mut app, caster, &jab);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::Blocked]);

    let jab_entity = ability_entity(&mut app, caster, &jab);
    let check = with_abilities(&mut app, move |a| {
        a.can_activate(jab_entity, TargetData::SelfCast)
    });
    assert_eq!(check, Err(ActivationError::Blocked));
}

fn channel(app: &mut App) -> Handle<AbilityDef> {
    ability(
        app,
        AbilityBuilder::new()
            .with_tag::<Magic>()
            .activation_owned_tag::<Casting>()
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    )
}

#[test]
fn cancel_ends_an_active_ability_and_returns_its_owned_tag() {
    let mut app = app();
    let channel = channel(&mut app);
    let caster = caster_with(&mut app, &[&channel]);
    record_all(&mut app);

    activate(&mut app, caster, &channel);
    ticks(&mut app, 2);
    let entity = ability_entity(&mut app, caster, &channel);
    assert!(with_abilities(&mut app, move |a| a.is_active(entity)));
    assert!(has::<Casting>(&app, caster));

    with_abilities(&mut app, move |a| a.cancel(entity));
    ticks(&mut app, 2);
    assert!(!has::<Casting>(&app, caster));
    let ended = recorded::<AbilityEnded>(&app);
    assert_eq!(ended.len(), 1);
    assert!(ended[0].cancelled);
}

#[test]
fn an_ability_that_runs_to_its_end_is_not_reported_cancelled() {
    let mut app = app();
    let spell = ability(
        &mut app,
        AbilityBuilder::new()
            .set_tasks(|| bsn! { Tasks [ @wait_task(0.25) ] })
            .build(),
    );
    let caster = caster_with(&mut app, &[&spell]);
    record_all(&mut app);

    activate(&mut app, caster, &spell);
    ticks(&mut app, ticks_for(0.25) - 3);
    assert!(
        recorded::<AbilityEnded>(&app).is_empty(),
        "the task is still running"
    );

    ticks(&mut app, 4);
    let ended = recorded::<AbilityEnded>(&app);
    assert_eq!(ended.len(), 1);
    assert!(
        !ended[0].cancelled,
        "a task that finished on its own is not a cancel"
    );
}

#[test]
fn activating_one_ability_cancels_others_by_tag() {
    let mut app = app();
    let channel = channel(&mut app);
    let interrupt = ability(
        &mut app,
        AbilityBuilder::new()
            .cancels_abilities_with::<Magic>()
            .build(),
    );
    let caster = caster_with(&mut app, &[&channel, &interrupt]);
    record_all(&mut app);

    activate(&mut app, caster, &channel);
    ticks(&mut app, 2);
    activate(&mut app, caster, &interrupt);
    ticks(&mut app, 2);

    let channel_entity = ability_entity(&mut app, caster, &channel);
    assert!(!with_abilities(&mut app, move |a| a.is_active(channel_entity)));
    assert!(
        recorded::<AbilityEnded>(&app)
            .iter()
            .any(|ended| ended.entity == channel_entity && ended.cancelled)
    );
}

#[test]
fn an_active_ability_blocks_others_by_tag() {
    let mut app = app();
    let focus = ability(
        &mut app,
        AbilityBuilder::new()
            .blocks_abilities_with::<Magic>()
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    );
    let spell = ability(&mut app, AbilityBuilder::new().with_tag::<Magic>().build());
    let caster = caster_with(&mut app, &[&focus, &spell]);
    record_all(&mut app);

    activate(&mut app, caster, &focus);
    ticks(&mut app, 2);
    activate(&mut app, caster, &spell);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::Blocked]);

    let focus_entity = ability_entity(&mut app, caster, &focus);
    with_abilities(&mut app, move |a| a.cancel(focus_entity));
    ticks(&mut app, 2);
    activate(&mut app, caster, &spell);
    ticks(&mut app, 1);
    assert_eq!(failures(&app).len(), 1, "the block lifts with the blocker");
}

#[test]
fn nested_tasks_run_after_their_parent() {
    let mut app = app();
    let combo = ability(
        &mut app,
        AbilityBuilder::new()
            .set_tasks(|| bsn! { Tasks [ @wait_task(0.25) Tasks [ @wait_task(0.25) ] ] })
            .build(),
    );
    let caster = caster_with(&mut app, &[&combo]);
    record_all(&mut app);

    activate(&mut app, caster, &combo);
    ticks(&mut app, ticks_for(0.35));
    assert!(
        recorded::<AbilityEnded>(&app).is_empty(),
        "ended before the nested task ran"
    );

    ticks(&mut app, ticks_for(0.25));
    assert_eq!(recorded::<AbilityEnded>(&app).len(), 1);
}

#[test]
fn revoke_removes_the_ability() {
    let mut app = app();
    let jab = ability(&mut app, AbilityBuilder::new().build());
    let caster = caster_with(&mut app, &[&jab]);

    let handle = jab.clone();
    assert!(with_abilities(&mut app, move |a| a.revoke(caster, &handle)));
    ticks(&mut app, 1);
    let handle = jab.clone();
    let left = with_abilities(&mut app, move |a| {
        a.get_ability_entity_by_handle(caster, &handle)
    });
    assert_eq!(left, None);
}
