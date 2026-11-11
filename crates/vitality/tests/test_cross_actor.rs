mod gameplay;

use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

/// v1 converges over a few ticks; the static-DAG resolver is meant to do it in the same tick.
const SETTLE: usize = 3;

fn set_power(app: &mut bevy::prelude::App, actor: bevy::prelude::Entity, value: f32) {
    with_vitality(app, move |v| v.set_base::<Power>(actor, value));
}

#[test]
fn a_live_aura_follows_its_casters_stat() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &aura);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);

    set_power(&mut app, caster, 30.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 30.0);
}

#[test]
fn a_live_modifier_reading_the_target_follows_the_target() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let scaling = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::dst(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &scaling);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);

    set_power(&mut app, target, 25.0);
    ticks(&mut app, SETTLE);
    assert_eq!(
        current::<Armor>(&app, target),
        25.0,
        "a dst() read must depend on the target, not the source"
    );

    set_power(&mut app, caster, 99.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 25.0);
}

#[test]
fn a_modifier_reading_both_actors_follows_both() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def().with::<Power>(1.0));
    let both = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(
                Power::src() + Power::dst(),
                ModOp::Add,
                EffectSubject::Target,
            )
            .build(),
    );
    apply(&mut app, caster, target, &both);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 11.0);

    set_power(&mut app, caster, 20.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 21.0);

    set_power(&mut app, target, 5.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 25.0);
}

#[test]
fn a_snapshot_ignores_later_changes() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let frozen = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify_snapshot::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &frozen);
    ticks(&mut app, SETTLE);

    set_power(&mut app, caster, 50.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);
}

#[test]
fn a_chain_of_actors_converges() {
    let mut app = app();
    let a = spawn(&mut app, hero_def());
    let b = spawn(&mut app, hero_def().with::<Power>(0.0));
    let c = spawn(&mut app, hero_def().with::<Power>(0.0));
    // b.Armor += a.Power, then c.Power += b.Armor
    let armor_from_power = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let power_from_armor = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(Armor::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, a, b, &armor_from_power);
    apply(&mut app, b, c, &power_from_armor);
    ticks(&mut app, SETTLE * 2);
    assert_eq!(current::<Power>(&app, c), 10.0);

    set_power(&mut app, a, 40.0);
    ticks(&mut app, SETTLE * 2);
    assert_eq!(current::<Armor>(&app, b), 40.0);
    assert_eq!(current::<Power>(&app, c), 40.0);
}
