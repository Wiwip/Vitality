use bevy::ecs::system::RunSystemOnce;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use vitality::ability::AbilityBuilder;
use vitality::actors::{Actor, ActorBuilder};
use vitality::ability::AbilityDef;
use vitality::condition::IsAttributeWithinBounds;
use vitality::context::Vitality;
use vitality::effect::{Effect, EffectInactive};
use vitality::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::registry::ability_registry::AbilityToken;
use vitality::registry::effect_registry::EffectToken;
use vitality::registry::{Registry, RegistryMut};
use vitality::{AttributesPlugin, attribute};

attribute!(TestA, f32);
attribute!(TestB, f64);

#[derive(Component, Copy, Clone, Debug, PartialEq)]
struct ConditionTag;

fn prepare_actor(mut ctx: Vitality, registry: Registry) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("TestActor".into())
            .with::<TestA>(0.0)
            .with::<TestB>(1.0)
            .grant_ability(&registry.ability(&TEST_ABILITY_TOKEN))
            .with_effect(&registry.effect(&CONDITION_EFFECT))
            .build(),
    );
    ctx.spawn_actor_from_handle(&actor_template);
}

fn prepare_effects(mut registry: RegistryMut) {
    registry.add_effect(
        TEST_EFFECT,
        Effect::permanent()
            .name("Increase Effect".into())
            .modify::<TestA>(100.0_f32, ModOp::Add, EffectSubject::Source)
            .build(),
    );

    registry.add_effect(
        CONDITION_EFFECT,
        Effect::permanent()
            .name("Condition Effect".into())
            // Active when TestA is more than 50.0
            .active_while(IsAttributeWithinBounds::<TestA>::source(50.0..))
            .insert(ConditionTag)
            .build(),
    );
}

pub const TEST_EFFECT: EffectToken = EffectToken::new_static("test.test");
pub const CONDITION_EFFECT: EffectToken = EffectToken::new_static("test.condition");

fn prepare_abilities(mut abilities: RegistryMut) {
    abilities.add_ability(TEST_ABILITY_TOKEN, fireball_ability());
}

pub const TEST_ABILITY_TOKEN: AbilityToken = AbilityToken::new_static("test.test");

pub fn fireball_ability() -> AbilityDef {
    AbilityBuilder::new()
        .with_name("Test Ability".into())
        .with_cooldown(3.5)
        .with_cost::<TestB>(3.0)
        .build()
}

#[test]
fn test_attribute_condition() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        LogPlugin {
            filter: "error,vitality=debug".into(),
            level: bevy::log::Level::DEBUG,
            ..default()
        },
        AttributesPlugin,
    ));
    app.add_plugins((
        vitality::init_attribute::<TestA>,
        vitality::init_attribute::<TestB>,
    ));
    // A headless update lasts microseconds, so with the real clock FixedUpdate never runs.
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(
        Startup,
        (prepare_effects, prepare_abilities, prepare_actor).chain(),
    );

    app.update();

    let mut query = app.world_mut().query::<(Entity, &Actor, &TestA, &TestB)>();
    let actor_id = query.single(app.world()).unwrap().0;

    // Check that the effect is inactive
    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, &ConditionTag, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().3;
    assert!(opt_inactive.is_some());

    app.world_mut()
        .run_system_once(move |mut ctx: Vitality, registry: Registry| {
            ctx.apply_effect_to_self(actor_id, &registry.effect(&TEST_EFFECT));
        })
        .unwrap();

    app.update();
    app.update();

    // Check that the effect is active
    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, &ConditionTag, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().3;
    assert!(opt_inactive.is_none());
}
