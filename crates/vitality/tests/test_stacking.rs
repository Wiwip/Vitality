mod gameplay;

use bevy::prelude::Assets;
use gameplay::*;
use vitality::effect::EffectStackingPolicy;
use vitality::effect::definition::EffectDef;
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
fn refresh_policy_reapplies_fire_effect_applied() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::for_seconds(2.0)
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
            .build(),
    );
    record::<EffectApplied>(&mut app);

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);

    let held = with_effects(&mut app, move |e| e.on(hero));
    let applied = recorded::<EffectApplied>(&app);
    assert_eq!(applied.len(), 2, "a refresh is an apply too");
    assert_eq!(applied[0].effect, Some(held[0]));
    assert_eq!(applied[1].effect, Some(held[0]));
}

/// The guard sits below the `Instant` branch, and an instant application spawns no entity of its
/// own, so the ordering only bites while the target still carries an instance of the handle.
#[test]
fn instant_effect_with_a_stacking_policy_applies_but_never_stacks() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let drain = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Mana>(-10.0, ModOp::Add, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::Add {
                count: 1,
                max_stack: 3,
            })
            .build(),
    );
    record::<EffectApplied>(&mut app);
    record::<EffectStacksChanged>(&mut app);

    apply(&mut app, hero, hero, &drain);
    ticks(&mut app, 1);
    let held = with_effects(&mut app, move |e| e.on(hero))[0];
    assert_eq!(
        base::<Mana>(&app, hero),
        50.0,
        "a buff never touches the base"
    );

    // the same asset reloaded as instant, with its permanent instance still applied
    app.world_mut()
        .resource_mut::<Assets<EffectDef>>()
        .get_mut(&drain)
        .expect("the effect asset is loaded")
        .application_policy = EffectApplicationPolicy::Instant;

    apply(&mut app, hero, hero, &drain);
    ticks(&mut app, 1);

    assert_eq!(
        base::<Mana>(&app, hero),
        40.0,
        "the instant application must still write its modifier"
    );
    assert_eq!(
        with_effects(&mut app, move |e| e.stacks(held)),
        Some(1),
        "an instant application never adds a stack"
    );
    let changes: Vec<(u32, u32)> = recorded::<EffectStacksChanged>(&app)
        .iter()
        .map(|change| (change.old, change.new))
        .collect();
    assert_eq!(
        changes,
        Vec::new(),
        "the instant branch must not queue a stack event"
    );
    let applied = recorded::<EffectApplied>(&app);
    assert_eq!(applied.len(), 2);
    assert_eq!(applied[0].effect, Some(held));
    assert_eq!(
        applied[1].effect, None,
        "the instant application spawns no entity"
    );
    assert_eq!(with_effects(&mut app, move |e| e.on(hero)).len(), 1);
}

#[test]
fn add_policy_keeps_one_instance_of_a_handle_per_actor() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let enemy = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        power_buff(EffectStackingPolicy::Add {
            count: 1,
            max_stack: 3,
        }),
    );

    apply(&mut app, enemy, hero, &buff);
    apply(&mut app, enemy, hero, &buff);
    apply(&mut app, hero, enemy, &buff);
    ticks(&mut app, 1);

    let on_hero = with_effects(&mut app, move |e| e.on(hero));
    let on_enemy = with_effects(&mut app, move |e| e.on(enemy));
    assert_eq!(
        on_hero.len(),
        1,
        "the second apply spawned a second instance instead of stacking on the first"
    );
    assert_eq!(
        on_enemy.len(),
        1,
        "the same handle on a second actor has to get its own instance"
    );
    assert_ne!(
        on_hero[0], on_enemy[0],
        "one effect entity is shared by two actors"
    );
    assert_eq!(
        with_effects(&mut app, move |e| e.stacks(on_hero[0])),
        Some(2),
        "the hero's instance owes it two stacks"
    );
    assert_eq!(
        with_effects(&mut app, move |e| e.stacks(on_enemy[0])),
        Some(1),
        "the hero's applies stacked onto the enemy's instance"
    );
    assert_eq!(current::<Power>(&app, hero), 20.0);
    assert_eq!(current::<Power>(&app, enemy), 15.0);
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
