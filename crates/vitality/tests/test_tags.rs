mod gameplay;

use gameplay::*;
use vitality::effect::conditions::HasComponent;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

fn burn() -> vitality::effect::definition::EffectDef {
    EffectBuilder::permanent()
        .with_tag::<Burning>()
        .modify::<Armor>(-2.0, ModOp::Add, EffectSubject::Target)
        .build()
}

#[test]
fn overlapping_grants_keep_the_tag_until_the_last_one_goes() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let burn = effect(&mut app, burn());

    apply(&mut app, hero, hero, &burn);
    apply(&mut app, hero, hero, &burn);
    ticks(&mut app, 1);
    assert!(has::<Burning>(&app, hero));

    let first = with_effects(&mut app, move |e| e.on(hero))[0];
    with_effects(&mut app, move |e| e.remove(first));
    ticks(&mut app, 1);
    assert!(has::<Burning>(&app, hero), "one burn is still on");

    with_effects(&mut app, move |e| e.remove_with_tag::<Burning>(hero));
    ticks(&mut app, 1);
    assert!(!has::<Burning>(&app, hero));
}

#[test]
fn an_immune_target_blocks_the_effect() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let burn = effect(
        &mut app,
        EffectBuilder::permanent()
            .with_tag::<Burning>()
            .blocked_by_tag::<Immune>()
            .modify::<Armor>(-2.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectBlocked>(&mut app);

    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    apply(&mut app, hero, hero, &burn);
    ticks(&mut app, 1);

    assert_eq!(current::<Armor>(&app, hero), 0.0);
    assert!(!has::<Burning>(&app, hero));
    assert_eq!(recorded::<EffectBlocked>(&app).len(), 1);

    with_vitality(&mut app, move |v| v.revoke_tag::<Immune>(hero));
    apply(&mut app, hero, hero, &burn);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), -2.0);
}

#[test]
fn immunity_gained_while_the_effect_runs_removes_it() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let bystander = spawn(&mut app, hero_def());
    let burn = effect(
        &mut app,
        EffectBuilder::permanent()
            .with_tag::<Burning>()
            .blocked_by_tag::<Immune>()
            .modify::<Armor>(-2.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectRemoved>(&mut app);
    record::<EffectApplied>(&mut app);

    apply(&mut app, hero, hero, &burn);
    apply(&mut app, bystander, bystander, &burn);
    ticks(&mut app, 1);
    assert!(has::<Burning>(&app, hero));

    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    ticks(&mut app, 1);

    assert!(
        with_effects(&mut app, move |e| e.on(hero)).is_empty(),
        "immunity takes the running effect off"
    );
    assert_eq!(current::<Armor>(&app, hero), 0.0);
    let removed = recorded::<EffectRemoved>(&app);
    let immunity: Vec<&EffectRemoved> = removed
        .iter()
        .filter(|removed| removed.reason == EffectRemovalReason::BlockedByTag)
        .collect();
    assert_eq!(
        immunity.len(),
        1,
        "one EffectRemoved carries the immunity reason"
    );
    assert_eq!(immunity[0].entity, hero);
    assert!(
        has::<Burning>(&app, bystander),
        "the target that never gained the tag keeps the effect"
    );
    assert_eq!(current::<Armor>(&app, bystander), -2.0);

    with_vitality(&mut app, move |v| v.revoke_tag::<Immune>(hero));
    ticks(&mut app, 1);

    assert!(
        with_effects(&mut app, move |e| e.on(hero)).is_empty(),
        "losing the immunity tag does not bring the effect back"
    );
    assert_eq!(current::<Armor>(&app, hero), 0.0);
    let hero_applied = recorded::<EffectApplied>(&app)
        .into_iter()
        .filter(|applied| applied.entity == hero)
        .count();
    assert_eq!(
        hero_applied, 1,
        "the blocked effect is gone for good, not replayed"
    );
}

#[test]
fn a_required_tag_gates_the_effect() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let combo = effect(
        &mut app,
        EffectBuilder::instant()
            .requires_tag::<Burning>()
            .modify::<Health>(-10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectBlocked>(&mut app);

    apply(&mut app, hero, hero, &combo);
    ticks(&mut app, 1);
    assert_eq!(base::<Health>(&app, hero), 100.0);
    assert_eq!(recorded::<EffectBlocked>(&app).len(), 1);

    with_vitality(&mut app, move |v| v.grant_tag::<Burning>(hero));
    apply(&mut app, hero, hero, &combo);
    ticks(&mut app, 1);
    assert_eq!(base::<Health>(&app, hero), 90.0);
    assert_eq!(recorded::<EffectBlocked>(&app).len(), 1);
}

#[test]
fn a_cleanse_removes_tagged_effects() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let burn = effect(&mut app, burn());
    let water = effect(
        &mut app,
        EffectBuilder::instant().cleanses::<Burning>().build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, hero, hero, &burn);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), -2.0);

    apply(&mut app, hero, hero, &water);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), 0.0);
    assert!(!has::<Burning>(&app, hero));
    let removed = recorded::<EffectRemoved>(&app);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].reason, EffectRemovalReason::Cleansed);
}

#[test]
fn a_cleanse_that_fails_its_condition_strips_nothing() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let burn = effect(&mut app, burn());
    let water = effect(
        &mut app,
        EffectBuilder::instant()
            .cleanses::<Burning>()
            .active_while(HasComponent::<Immune>::target())
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, hero, hero, &burn);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), -2.0);

    apply(&mut app, hero, hero, &water);
    ticks(&mut app, 1);

    assert!(has::<Burning>(&app, hero), "a gated apply strips nothing");
    assert_eq!(current::<Armor>(&app, hero), -2.0);
    assert_eq!(
        recorded::<EffectRemoved>(&app).len(),
        0,
        "a gated apply fires no EffectRemoved"
    );
}

#[test]
fn a_persistent_cleanse_that_fails_its_attach_condition_strips_nothing() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let burn = effect(&mut app, burn());
    let water = effect(
        &mut app,
        EffectBuilder::permanent()
            .cleanses::<Burning>()
            .attach_if(HasComponent::<Immune>::target())
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, hero, hero, &burn);
    ticks(&mut app, 1);
    assert_eq!(current::<Armor>(&app, hero), -2.0);

    apply(&mut app, hero, hero, &water);
    ticks(&mut app, 1);

    assert!(has::<Burning>(&app, hero), "a gated apply strips nothing");
    assert_eq!(current::<Armor>(&app, hero), -2.0);
    assert_eq!(
        recorded::<EffectRemoved>(&app).len(),
        0,
        "a gated apply fires no EffectRemoved"
    );
}
