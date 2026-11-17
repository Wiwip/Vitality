mod gameplay;

use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

fn clamped_hero() -> vitality::actors::ActorBuilder {
    hero_def().clamp::<Health>(0.0, MaxHealth::src())
}

#[test]
fn health_follows_a_lowered_max() {
    let mut app = app();
    let hero = spawn(&mut app, clamped_hero());
    ticks(&mut app, 1);
    assert_eq!(current::<Health>(&app, hero), 100.0);

    let curse = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<MaxHealth>(-40.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, hero, hero, &curse);
    ticks(&mut app, 3);

    assert_eq!(current::<MaxHealth>(&app, hero), 60.0);
    assert_eq!(
        current::<Health>(&app, hero),
        60.0,
        "Health stayed above its max"
    );
    assert_eq!(base::<Health>(&app, hero), 60.0, "the base is clamped too");
}

/// A limit that is an expression, not a bare attribute read, still has to pull the clamped attribute.
#[test]
fn health_follows_a_lowered_computed_limit() {
    let mut app = app();
    let hero = spawn(
        &mut app,
        vitality::actors::ActorBuilder::new()
            .name("Hero")
            .with::<Health>(80.0)
            .with::<MaxHealth>(100.0)
            .clamp::<Health>(0.0, MaxHealth::src() * 0.5),
    );
    assert_eq!(current::<Health>(&app, hero), 50.0, "half of MaxHealth");

    with_vitality(&mut app, move |v| v.set_base::<MaxHealth>(hero, 60.0));
    ticks(&mut app, 3);

    assert_eq!(current::<MaxHealth>(&app, hero), 60.0);
    assert_eq!(
        current::<Health>(&app, hero),
        30.0,
        "Health kept the limit it read before MaxHealth moved"
    );
}

#[test]
fn a_buff_cannot_push_current_past_the_max() {
    let mut app = app();
    let hero = spawn(&mut app, clamped_hero());
    let overheal = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(50.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, hero, hero, &overheal);
    ticks(&mut app, 3);

    assert_eq!(current::<Health>(&app, hero), 100.0);
}

#[test]
fn damage_below_zero_stops_at_the_min() {
    let mut app = app();
    let hero = spawn(&mut app, clamped_hero());
    let nuke = effect(
        &mut app,
        EffectBuilder::instant()
            .modify::<Health>(-500.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, hero, hero, &nuke);
    ticks(&mut app, 2);

    assert_eq!(base::<Health>(&app, hero), 0.0);
    assert_eq!(current::<Health>(&app, hero), 0.0);
}

#[test]
fn raising_the_max_keeps_health_where_it_was() {
    let mut app = app();
    let hero = spawn(&mut app, clamped_hero());
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<MaxHealth>(50.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, hero, hero, &blessing);
    ticks(&mut app, 3);

    assert_eq!(current::<MaxHealth>(&app, hero), 150.0);
    assert_eq!(current::<Health>(&app, hero), 100.0);
}
