mod gameplay;

use gameplay::*;
use vitality::effect::EffectStackingPolicy;
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::store::Assets;

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
fn two_removals_in_one_system_both_count() {
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

    with_effects(&mut app, move |e| {
        e.remove_stacks(effect_entity, 1);
        e.remove_stacks(effect_entity, 1);
    });
    ticks(&mut app, 1);

    assert_eq!(
        with_effects(&mut app, move |e| e.stacks(effect_entity)),
        Some(1),
        "each removal takes its own stack off"
    );
    assert_eq!(current::<Power>(&app, hero), 15.0);
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

/// two ticks is the shortest duration whose expiry tick a second apply can still land on
#[test]
fn a_refresh_on_the_expiry_tick_beats_the_despawn() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::for_seconds(2.0 * STEP.as_secs_f32())
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
            .build(),
    );
    record::<EffectRemoved>(&mut app);

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);

    assert_eq!(
        current::<Power>(&app, hero),
        15.0,
        "a reapply landing on the tick the duration runs out on has to refresh the copy before \
         the duration walk reaps it"
    );
    assert_eq!(with_effects(&mut app, move |e| e.on(hero)).len(), 1);
    assert!(
        recorded::<EffectRemoved>(&app).is_empty(),
        "a duration that was refreshed on its last tick did not expire"
    );
}

/// 0.3 s is 19.2 of the harness' 1/64 s ticks, so the ticker finishes on ticks 20, 39, 58, 77 and
/// 97, and the refresh after 36 ticks moves the expiry to tick 101: five fires, the last of them
/// four ticks short of the new expiry.
const FIRES_BY_THE_REFRESHED_EXPIRY: f32 = 5.0;

#[test]
fn a_refresh_keeps_the_fire_due_before_the_refreshed_expiry() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let drip = effect(
        &mut app,
        EffectBuilder::every_second_for_duration(0.3, 1.0)
            .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
            .build(),
    );

    apply(&mut app, hero, hero, &drip);
    ticks(&mut app, ticks_for(0.5625));
    apply(&mut app, hero, hero, &drip);
    ticks(&mut app, ticks_for(2.0));

    assert_eq!(
        base::<Health>(&app, hero),
        FIRES_BY_THE_REFRESHED_EXPIRY,
        "the refresh restarts the duration but not the ticker, so the cap has to count the owed \
         fires off the ticker's own phase and not off the duration's elapsed time",
    );
    assert_eq!(
        with_effects(&mut app, move |e| e.on(hero)).len(),
        0,
        "the refreshed duration still expires"
    );
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

/// The guard sits below the `Instant` branch, so the ordering only bites while an instance is carried
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
