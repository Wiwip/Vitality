mod gameplay;

use gameplay::*;
use vitality::attributes::{Clamp, ManageAttributes};
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
    assert_eq!(
        base::<Armor>(&app, hero),
        7.0,
        "and nothing was written on the target at all"
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
