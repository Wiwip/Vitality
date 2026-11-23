mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
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

/// a computed limit's edge is found mid-tick, after its attribute was walked, so it is walked again
#[test]
fn a_clamp_edge_found_mid_tick_re_resolves_the_attribute_it_governs() {
    let mut app = app();
    let mut def = Some(
        vitality::actors::ActorBuilder::new()
            .name("Hero")
            .with::<Mana>(80.0)
            .with::<MaxHealth>(100.0)
            .clamp::<Mana>(0.0, MaxHealth::src() * 0.5)
            .build(),
    );
    let hero = with_vitality(&mut app, move |v| {
        v.add_spawn_actor(def.take().expect("spawned once")).id()
    });
    with_vitality(&mut app, move |v| v.set_base::<MaxHealth>(hero, 60.0));

    ticks(&mut app, 1);
    assert_eq!(current::<MaxHealth>(&app, hero), 60.0);
    assert_eq!(
        current::<Mana>(&app, hero),
        50.0,
        "the first resolve clamped against half of 100, not half of the 60 written the same tick"
    );
    assert_eq!(
        base::<Mana>(&app, hero),
        50.0,
        "the missed limit is folded in"
    );

    ticks(&mut app, 1);
    assert_eq!(
        current::<Mana>(&app, hero),
        30.0,
        "the edge that was new marks Mana again, so the next walk clamps against half of 60"
    );

    with_vitality(&mut app, move |v| v.set_base::<Mana>(hero, 40.0));
    ticks(&mut app, 1);
    assert_eq!(
        current::<Mana>(&app, hero),
        30.0,
        "the limit the edge found keeps clamping every later write"
    );
}

/// a min above the max is legal input (`clamp` takes any two expressions), so the clamp has to land on the max
#[test]
fn an_inverted_clamp_lands_on_the_max_and_says_so() {
    let logs = Logs::of_binary();
    let mut app = app();
    let hero = spawn(
        &mut app,
        hero_def()
            .with::<Health>(150.0)
            .clamp::<Health>(200.0, 100.0),
    );
    ticks(&mut app, 2);

    assert_eq!(current::<Health>(&app, hero), 100.0, "above the max");
    assert_eq!(base::<Health>(&app, hero), 100.0, "the base is clamped too");

    let warns = logs.warns();
    assert!(
        warns.contains("Health") && warns.contains("200") && warns.contains("100"),
        "the inverted limits have to be named with the attribute: {warns}"
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

/// A limit whose expression closes a cycle is refused at registration, not from inside a tick.
#[test]
fn a_clamp_limit_that_closes_a_cycle_is_refused_rather_than_panicking_the_tick() {
    let logs = Logs::of_binary();
    let mut app = app();
    let hero = spawn(
        &mut app,
        hero_def()
            .with::<Power>(200.0)
            .with::<Armor>(50.0)
            // a bare limit is known at spawn, so Armor -> Power is already in the graph
            .clamp::<Power>(0.0, Armor::src())
            // a computed limit reports Power on its first pass, so it would close Armor -> Power -> Armor
            .clamp::<Armor>(0.0, Power::src() * 2.0),
    );
    ticks(&mut app, 2);

    assert_eq!(
        current::<Power>(&app, hero),
        50.0,
        "the edge that is legal still clamps Power to its limit"
    );
    assert_eq!(current::<Armor>(&app, hero), 50.0);

    let warns = logs.warns();
    assert!(
        warns.contains("closes an attribute dependency cycle")
            && warns.contains("Power")
            && warns.contains("Armor"),
        "the refused edge has to name both attributes and why: {warns}"
    );

    with_vitality(&mut app, move |v| v.set_base::<Power>(hero, 20.0));
    ticks(&mut app, 1);
    assert_eq!(
        current::<Power>(&app, hero),
        20.0,
        "the refused edge left the order usable"
    );
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
