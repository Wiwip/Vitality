mod gameplay;

use gameplay::*;
use vitality::attributes::{Clamp, ManageAttributes};
use vitality::effect::EffectStackingPolicy;
use vitality::effect::conditions::HasComponent;
use vitality::effect::context::EffectRole;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

#[test]
fn an_effect_folds_into_an_entity_that_is_not_an_actor() {
    let mut app = app();
    let post = app.world_mut().spawn(Health::new(100.0)).id();
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, post, post, &blessing);
    ticks(&mut app, 1);

    assert!(
        !has::<ManageAttributes>(&app, post),
        "the point of the cell is that nothing had to be added to the entity to fold into"
    );
    assert_eq!(
        with_effects(&mut app, move |e| e.on(post)).len(),
        1,
        "the effect is listed on the entity"
    );
    assert_eq!(
        base::<Health>(&app, post),
        100.0,
        "a fold never moves the base"
    );
    assert_eq!(
        current::<Health>(&app, post),
        110.0,
        "the entity's own attribute takes the +10 the same way an actor's would"
    );
}

#[test]
fn a_base_write_on_a_non_actor_follows_in_one_tick() {
    let mut app = app();
    let post = app.world_mut().spawn(Health::new(100.0)).id();
    let hero = spawn(&mut app, hero_def());
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, post, post, &blessing);
    apply(&mut app, hero, hero, &blessing);
    ticks(&mut app, 1);

    with_vitality(&mut app, move |v| {
        v.set_base::<Health>(post, 200.0);
        v.set_base::<Health>(hero, 200.0);
    });
    ticks(&mut app, 1);

    assert_eq!(
        base::<Health>(&app, post),
        200.0,
        "the write moved the base"
    );
    assert_eq!(
        current::<Health>(&app, post),
        210.0,
        "the carrier's current value follows the new base, so the write is not dropped"
    );
    assert_eq!(
        current::<Health>(&app, hero),
        210.0,
        "control: the actor leg follows the same way it did before"
    );
}

#[test]
fn an_effect_subject_modifier_writes_the_effect_entity() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Armor>(7.0));
    let buff = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(5.0, ModOp::Add, EffectSubject::Effect)
            .build(),
    );

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);

    let effect_entity = with_effects(&mut app, move |effects| effects.on(hero))[0];

    assert_eq!(
        current::<Armor>(&app, effect_entity),
        5.0,
        "the effect entity carries the attribute the modifier names and folds its +5 into it"
    );
    assert_eq!(
        base::<Armor>(&app, effect_entity),
        0.0,
        "a fold writes the current value here too, leaving the base it was given"
    );
    assert_eq!(
        current::<Armor>(&app, hero),
        7.0,
        "the target keeps the Armor it came with: an Effect modifier no longer folds onto it"
    );
}

#[test]
fn an_effect_gated_off_by_active_while_drops_its_own_modifier() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(5.0, ModOp::Add, EffectSubject::Effect)
            .active_while(HasComponent::<Immune>::target())
            .build(),
    );
    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    let effect_entity = with_effects(&mut app, move |effects| effects.on(hero))[0];
    assert_eq!(
        current::<Armor>(&app, effect_entity),
        5.0,
        "the gate holds, so the effect folds its own +5"
    );

    with_vitality(&mut app, move |v| v.revoke_tag::<Immune>(hero));
    ticks(&mut app, 1);
    assert_eq!(
        current::<Armor>(&app, effect_entity),
        0.0,
        "a gated-off effect folds nothing, and the entity its `Effect` leg folds onto has to hear that"
    );

    with_vitality(&mut app, move |v| v.grant_tag::<Immune>(hero));
    ticks(&mut app, 1);
    assert_eq!(
        current::<Armor>(&app, effect_entity),
        5.0,
        "the gate reopens, so the +5 is folded again"
    );
}

#[test]
fn a_stacked_effect_scales_its_own_leg_on_the_effect_entity() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let buff = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(5.0, ModOp::Add, EffectSubject::Effect)
            .with_stacking_policy(EffectStackingPolicy::Add {
                count: 1,
                max_stack: 3,
            })
            .build(),
    );

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    let effect_entity = with_effects(&mut app, move |effects| effects.on(hero))[0];
    assert_eq!(
        current::<Armor>(&app, effect_entity),
        5.0,
        "one stack folds its own +5 once"
    );

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    assert_eq!(
        with_effects(&mut app, move |e| e.stacks(effect_entity)),
        Some(2),
        "the second apply stacked on the same effect entity"
    );
    assert_eq!(
        current::<Armor>(&app, effect_entity),
        10.0,
        "two stacks scale the effect's own leg the same way they scale the target leg"
    );
}

#[test]
fn a_periodic_effect_subject_modifier_follows_the_effect_entity() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def().with::<Armor>(7.0));
    let scaling = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.5)
            .modify::<Armor>(
                Armor::at::<EffectRole, EffectExprSchema>(),
                ModOp::Add,
                EffectSubject::Effect,
            )
            .build(),
    );

    apply(&mut app, caster, caster, &scaling);
    ticks(&mut app, 1);

    let effect_entity = with_effects(&mut app, move |effects| effects.on(caster))[0];
    with_vitality(&mut app, move |v| {
        v.set_base::<Armor>(effect_entity, 20.0);
    });
    ticks(&mut app, 3);

    assert_eq!(
        current::<Armor>(&app, effect_entity),
        20.0,
        "the effect entity carries the attribute its modifier names, and a ticking effect folds nothing before it fires"
    );

    ticks(&mut app, ticks_for(0.5));
    assert_eq!(
        current::<Armor>(&app, effect_entity),
        40.0,
        "the fire pays the effect entity's own 20"
    );

    with_vitality(&mut app, move |v| {
        v.set_base::<Armor>(caster, 50.0);
    });
    ticks(&mut app, ticks_for(0.5));

    assert_eq!(
        current::<Armor>(&app, effect_entity),
        80.0,
        "the second fire re-reads the effect entity's own 40, so the caster's new 50 never reaches it"
    );
}

#[test]
fn a_clamp_on_a_non_actor_bounds_the_folded_value() {
    let mut app = app();
    let post = app
        .world_mut()
        .spawn((
            Health::new(100.0),
            Clamp::<Health> {
                min_limit: 0.0,
                max_limit: 105.0,
                ready: true,
            },
        ))
        .id();
    let overheal = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, post, post, &overheal);
    ticks(&mut app, 1);

    assert_eq!(
        current::<Health>(&app, post),
        105.0,
        "the carrier's own clamp bounds its fold the way it bounds an actor's"
    );
}
