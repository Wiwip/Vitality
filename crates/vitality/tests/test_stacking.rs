mod gameplay;

use gameplay::*;
use vitality::effect::EffectStackingPolicy;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

fn power_buff(policy: EffectStackingPolicy) -> vitality::effect::definition::EffectDef {
    EffectBuilder::permanent()
        .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
        .with_stacking_policy(policy)
        .build()
}

#[test]
fn add_policy_stacks_up_to_the_cap_and_scales_the_modifier() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        power_buff(EffectStackingPolicy::Add {
            count: 1,
            max_stack: 3,
        }),
    );
    record::<EffectStacksChanged>(&mut app);

    for _ in 0..4 {
        apply(&mut app, hero, hero, &buff);
        ticks(&mut app, 1);
    }

    let effects = with_effects(&mut app, move |effects| effects.on(hero));
    assert_eq!(effects.len(), 1, "stacking reuses the first effect");
    let stacks = with_effects(&mut app, move |e| e.stacks(effects[0]));
    assert_eq!(stacks, Some(3), "capped at max_stack");
    assert_eq!(current::<Power>(&app, hero), 10.0 + 3.0 * 5.0);

    let changes: Vec<_> = recorded::<EffectStacksChanged>(&app)
        .iter()
        .map(|change| (change.old, change.new))
        .collect();
    assert_eq!(
        changes,
        vec![(1, 2), (2, 3)],
        "the capped apply changes nothing"
    );
}

#[test]
fn removing_stacks_lowers_the_value_and_the_last_stack_removes_the_effect() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        power_buff(EffectStackingPolicy::Add {
            count: 1,
            max_stack: 5,
        }),
    );
    for _ in 0..3 {
        apply(&mut app, hero, hero, &buff);
        ticks(&mut app, 1);
    }
    let effect_entity = with_effects(&mut app, move |e| e.on(hero))[0];

    with_effects(&mut app, move |e| e.remove_stacks(effect_entity, 2));
    ticks(&mut app, 1);
    assert_eq!(current::<Power>(&app, hero), 15.0);

    with_effects(&mut app, move |e| e.remove_stacks(effect_entity, 1));
    ticks(&mut app, 1);
    assert_eq!(current::<Power>(&app, hero), 10.0);
    assert!(with_effects(&mut app, move |e| e.on(hero)).is_empty());
}

#[test]
fn none_policy_applies_independent_copies() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(&mut app, power_buff(EffectStackingPolicy::None));

    apply(&mut app, hero, hero, &buff);
    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);

    assert_eq!(with_effects(&mut app, move |e| e.on(hero)).len(), 2);
    assert_eq!(current::<Power>(&app, hero), 20.0);
}

#[test]
fn refresh_policy_restarts_the_duration() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::for_seconds(0.5)
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
            .build(),
    );

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, ticks_for(0.4));
    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, ticks_for(0.4));
    assert_eq!(
        current::<Power>(&app, hero),
        15.0,
        "the refresh should have kept it"
    );
    assert_eq!(with_effects(&mut app, move |e| e.on(hero)).len(), 1);

    ticks(&mut app, ticks_for(0.2));
    assert_eq!(current::<Power>(&app, hero), 10.0);
}

#[test]
fn periodic_damage_scales_with_stacks() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let poison = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.5)
            .modify::<Health>(-2.0, ModOp::Add, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::Add {
                count: 1,
                max_stack: 10,
            })
            .build(),
    );

    apply(&mut app, hero, hero, &poison);
    apply(&mut app, hero, hero, &poison);
    ticks(&mut app, ticks_for(0.5) + 1);

    assert_eq!(base::<Health>(&app, hero), 96.0, "one tick at two stacks");
}
