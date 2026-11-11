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
