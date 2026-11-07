//! Runs effects and abilities together in one app, so any system access conflict between the
//! plugins shows up when the schedules are built and run.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;
use vitality::ability::{Abilities, AbilityBuilder, TargetData};
use vitality::actors::ActorBuilder;
use vitality::condition::IsAttributeWithinBounds;
use vitality::context::Vitality;
use vitality::effect::Effect;
use vitality::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::registry::ability_registry::AbilityToken;
use vitality::registry::effect_registry::EffectToken;
use vitality::registry::{Registry, RegistryMut};
use vitality::{AttributesPlugin, attribute};

attribute!(Health, f32);
attribute!(Armor, f32);

const BUFF: EffectToken = EffectToken::new_static("smoke.buff");
const GATED: EffectToken = EffectToken::new_static("smoke.gated");
const SKILL: AbilityToken = AbilityToken::new_static("smoke.skill");

#[derive(Component, Clone)]
struct Hero;

fn setup(mut registry: RegistryMut) {
    registry.add_effect(
        BUFF,
        Effect::permanent()
            .name("Buff".into())
            .modify::<Health>(50.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    registry.add_effect(
        GATED,
        Effect::permanent()
            .name("Gated".into())
            .modify::<Armor>(5.0, ModOp::Add, EffectSubject::Target)
            .active_while(IsAttributeWithinBounds::<Health>::target(120.0..))
            .build(),
    );
    registry.add_ability(
        SKILL,
        AbilityBuilder::new()
            .with_name("Skill".into())
            .with_cooldown(0.1)
            .build(),
    );
}

fn spawn(mut ctx: Vitality, registry: Registry) {
    ctx.add_spawn_actor(
        ActorBuilder::new()
            .name("Hero")
            .insert(Hero)
            .with::<Health>(100.0)
            .with::<Armor>(0.0)
            .with_effect(&registry.effect(&BUFF))
            .with_effect(&registry.effect(&GATED))
            .build(),
    );
}

#[test]
fn effects_and_abilities_run_together() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(vitality::init_attribute::<Health>);
    app.add_plugins(vitality::init_attribute::<Armor>);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(20)));
    app.add_systems(Startup, (setup, spawn).chain());

    app.update();
    let hero = app
        .world_mut()
        .query_filtered::<Entity, With<Hero>>()
        .single(app.world())
        .unwrap();

    for _ in 0..4 {
        app.update();
    }

    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| {
            let handle = abilities.get_ability_from_token(&SKILL);
            abilities.grant_ability(&handle, hero).unwrap();
        })
        .unwrap();
    app.update();
    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| {
            abilities.try_activate_by_token(hero, &SKILL, TargetData::SelfCast)
        })
        .unwrap();
    for _ in 0..12 {
        app.update();
    }

    let health = app.world().get::<Health>(hero).unwrap();
    assert_eq!(health.current_value(), 150.0, "the buff applies once");
    let armor = app.world().get::<Armor>(hero).unwrap();
    assert_eq!(armor.current_value(), 5.0, "the gated effect activates after the buff");
}
