mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::CurrentValueChanged;
use vitality::effect::EffectStackingPolicy;
use vitality::effect::conditions::{ChanceCondition, ConditionRng};
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

#[derive(Resource, Default)]
struct Changes(Vec<(&'static str, Entity, u32, u32)>);

fn log<T: Attribute<Property = f32>>(app: &mut App, name: &'static str) {
    app.add_observer(
        move |trigger: On<CurrentValueChanged<T>>, mut changes: ResMut<Changes>| {
            let event = trigger.event();
            changes
                .0
                .push((name, event.entity, event.old.to_bits(), event.new.to_bits()));
        },
    );
}

type Run = (Vec<(&'static str, Entity, u32, u32)>, Vec<[u32; 5]>);

/// live cross-actor modifiers, stacked Mul, an expiring buff, a periodic drain and chance gates
fn run(seed: u64) -> Run {
    let mut app = app();
    app.insert_resource(ConditionRng::new(seed));
    app.init_resource::<Changes>();
    log::<Health>(&mut app, "Health");
    log::<MaxHealth>(&mut app, "MaxHealth");
    log::<Mana>(&mut app, "Mana");
    log::<Power>(&mut app, "Power");
    log::<Armor>(&mut app, "Armor");

    let actors: Vec<Entity> = (0..6)
        .map(|i| spawn(&mut app, hero_def().with::<Power>(10.0 + i as f32)))
        .collect();
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(
                Power::src() + Power::dst(),
                ModOp::Add,
                EffectSubject::Target,
            )
            .build(),
    );
    let fury = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(1.1f32, ModOp::Mul, EffectSubject::Target)
            .with_stacking_policy(EffectStackingPolicy::Add {
                count: 1,
                max_stack: 5,
            })
            .build(),
    );
    let haste = effect(
        &mut app,
        EffectBuilder::for_seconds(0.1)
            .modify::<Mana>(7.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let drain = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.05)
            .modify::<Health>(-1.5f32, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let lucky = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<MaxHealth>(Power::src(), ModOp::Add, EffectSubject::Target)
            .attach_if(ChanceCondition(0.5))
            .build(),
    );

    for (i, &actor) in actors.iter().enumerate() {
        let other = actors[(i + 1) % actors.len()];
        apply(&mut app, other, actor, &aura);
        apply(&mut app, actor, actor, &drain);
        apply(&mut app, actor, other, &haste);
        for _ in 0..=i % 3 {
            apply(&mut app, actor, actor, &fury);
        }
        apply(&mut app, other, actor, &lucky);
        apply(&mut app, actor, other, &lucky);
        ticks(&mut app, 1);
    }
    ticks(&mut app, 20);
    with_vitality(&mut app, {
        let first = actors[0];
        move |v| v.set_base::<Power>(first, 40.0)
    });
    ticks(&mut app, 20);

    let values = actors
        .iter()
        .map(|&a| {
            [
                current::<Health>(&app, a).to_bits(),
                current::<MaxHealth>(&app, a).to_bits(),
                current::<Mana>(&app, a).to_bits(),
                current::<Power>(&app, a).to_bits(),
                current::<Armor>(&app, a).to_bits(),
            ]
        })
        .collect();
    let changes = std::mem::take(&mut app.world_mut().resource_mut::<Changes>().0);
    (changes, values)
}

#[test]
fn the_same_seed_replays_the_same_bits_and_change_order() {
    let first = run(7);
    assert!(!first.0.is_empty());
    assert_eq!(first, run(7));
}

#[test]
fn the_seed_reaches_chance_conditions() {
    assert_ne!(run(11).1, run(12).1);
}
