//! what a game sees: the facade on `MinimalPlugins`, public API only, no `gameplay` helper

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;
use vitality::actors::ActorBuilder;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::{attribute, tag};

attribute!(Hp, f32);
attribute!(MaxHp, f32);
attribute!(Might, f32);

tag!(Burn);

// bevy's default fixed timestep, so one update is one fixed tick
const STEP: Duration = Duration::from_micros(15_625);

/// a burn that ticks once a second for five seconds, draining the caster's Might, plus the clamp it lands on
fn game() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        VitalityPlugin::new()
            .attribute::<Hp>()
            .attribute::<MaxHp>()
            .attribute::<Might>(),
    ))
    // a headless app drives FixedUpdate itself (README:85)
    .insert_resource(TimeUpdateStrategy::ManualDuration(STEP))
    .add_systems(Startup, spawn_fighter)
    .add_systems(Update, light_burn.run_if(run_once));
    app
}

fn spawn_fighter(mut vitality: Vitality) {
    vitality.add_spawn_actor(
        ActorBuilder::new()
            .name("Fighter")
            .with::<Hp>(100.0)
            .with::<MaxHp>(100.0)
            .with::<Might>(10.0)
            .clamp::<Hp>(0.0, MaxHp::src())
            .build(),
    );
}

fn light_burn(mut vitality: Vitality, fighter: Single<Entity, With<Hp>>) {
    let burn = vitality.add_effect(
        EffectBuilder::every_second_for_duration(1.0, 5.0)
            .with_tag::<Burn>()
            .modify::<Hp>(Might::src(), ModOp::Sub, EffectSubject::Target)
            .build(),
    );
    vitality.apply_effect_to_self(*fighter, &burn);
}

#[test]
fn a_burn_drains_the_attribute_and_its_tag_expires_with_it() {
    let mut app = game();
    app.update();
    let mut query = app.world_mut().query_filtered::<Entity, With<Hp>>();
    let fighter = query.iter(app.world()).next().expect("the actor spawned");

    // 6.25 s of fixed ticks: five burn ticks landed, and the effect is past its five seconds
    for _ in 0..400 {
        app.update();
    }

    let world = app.world();
    assert_eq!(
        world.get::<Hp>(fighter).unwrap().current_value(),
        50.0,
        "five ticks at Might 10 off 100, so the resolver is doing what the def says"
    );
    assert_eq!(
        world.get::<MaxHp>(fighter).unwrap().current_value(),
        100.0,
        "the burn never touches the clamp's upper bound"
    );
    assert!(
        world.get::<Burn>(fighter).is_none(),
        "the tag went away with the effect that granted it"
    );
}
