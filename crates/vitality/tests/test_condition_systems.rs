use bevy::ecs::system::RunSystemOnce;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use vitality::ability::AbilityBuilder;
use vitality::ability::AbilityDef;
use vitality::actors::{Actor, ActorBuilder};
use vitality::context::Vitality;
use vitality::effect::conditions::IsAttributeWithinBounds;
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::effect::{Effect, EffectInactive};
use vitality::prelude::*;
use vitality::registry::ability_registry::AbilityToken;
use vitality::registry::effect_registry::EffectToken;
use vitality::registry::{Registry, RegistryMut};
use vitality::store::Assets;
use vitality::{AttributesPlugin, attribute};

attribute!(TestA, f32);
attribute!(TestB, f64);

#[derive(Component, Copy, Clone, Debug, PartialEq)]
struct ConditionTag;

fn prepare_actor(mut ctx: Vitality, registry: Registry) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("TestActor")
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
            .active_while(IsAttributeWithinBounds::<TestA>::source(50.0..))
            .insert(ConditionTag)
            .build(),
    );
}

pub const TEST_EFFECT: EffectToken = EffectToken::new_static("test.test");
pub const CONDITION_EFFECT: EffectToken = EffectToken::new_static("test.condition");
pub const GATED_BUFF: EffectToken = EffectToken::new_static("test.gated_buff");

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

/// one condition on the target's own TestA, so a write can flip it without touching TestB
fn prepare_gated_effect(mut registry: RegistryMut) {
    registry.add_effect(
        GATED_BUFF,
        Effect::permanent()
            .name("Gated Buff".into())
            .modify::<TestB>(25.0_f64, ModOp::Add, EffectSubject::Target)
            .active_while(IsAttributeWithinBounds::<TestA>::target(..50.0_f32))
            .build(),
    );
}

fn prepare_gated_actor(mut ctx: Vitality, registry: Registry) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("GatedActor")
            .with::<TestA>(0.0)
            .with::<TestB>(1.0)
            .with_effect(&registry.effect(&GATED_BUFF))
            .build(),
    );
    ctx.spawn_actor_from_handle(&actor_template);
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

    // conditions run on the fixed clock, which has no tick in the first update
    app.update();
    app.update();

    let mut query = app.world_mut().query::<(Entity, &Actor, &TestA, &TestB)>();
    let actor_id = query.single(app.world()).unwrap().0;

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

    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, &ConditionTag, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().3;
    assert!(opt_inactive.is_none());
}

/// the same gated buff on an actor that starts outside the range, so the marker lands before anything
/// else touches the definition
fn prepare_stale_actor(mut ctx: Vitality, registry: Registry) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("StaleActor")
            .with::<TestA>(60.0)
            .with::<TestB>(1.0)
            .with_effect(&registry.effect(&GATED_BUFF))
            .build(),
    );
    ctx.spawn_actor_from_handle(&actor_template);
}

/// clearing the conditions of a loaded definition is not enough to reactivate the row it marked: the
/// fast path only skips an unmarked row, so the marker has to survive to the removal branch
#[test]
fn test_dropping_conditions_clears_a_stale_inactive_marker() {
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
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(Startup, (prepare_gated_effect, prepare_stale_actor).chain());

    app.update();
    app.update();

    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().2;
    assert!(
        opt_inactive.is_some(),
        "the buff starts inactive, so there is no stale marker to clear"
    );

    app.world_mut()
        .run_system_once(
            |mut defs: ResMut<Assets<EffectDef>>, rows: Query<&Effect>| {
                let handle = rows.single().unwrap().0;
                defs.get_mut(&handle).unwrap().activate_conditions.clear();
            },
        )
        .unwrap();

    for _ in 0..4 {
        app.update();
    }

    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().2;
    assert!(
        opt_inactive.is_none(),
        "the definition has no conditions left but the row still carries the marker"
    );

    let mut query = app.world_mut().query::<(Entity, &Actor, &TestB)>();
    let test_b = query.single(app.world()).unwrap().2;
    assert_eq!(
        test_b.current_value(),
        26.0,
        "the marker is gone but the unconditional buff still does not apply"
    );
}

/// flipping an effect off changes the fold without writing any attribute of the target, so only the
/// flip's own mark repaints it
#[test]
fn test_condition_deactivation_repaints_target() {
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
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(Startup, (prepare_gated_effect, prepare_gated_actor).chain());

    app.update();
    app.update();

    let mut query = app.world_mut().query::<(Entity, &Actor, &TestB)>();
    let (actor_id, _, test_b) = query.single(app.world()).unwrap();
    assert_eq!(
        test_b.current_value(),
        26.0,
        "the gated buff never applied while its condition held"
    );

    // A write TestB has no part in: the condition reads TestA, and TestB is not one of its dependents.
    app.world_mut()
        .run_system_once(move |mut attributes: Query<&mut TestA>| {
            attributes.single_mut().unwrap().set_base_value(60.0);
        })
        .unwrap();

    for _ in 0..4 {
        app.update();
    }

    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().2;
    assert!(
        opt_inactive.is_some(),
        "the condition never turned the buff off, so a TestB that stayed put would prove nothing"
    );

    let test_b = app.world().get::<TestB>(actor_id).unwrap();
    assert_eq!(
        test_b.current_value(),
        1.0,
        "the buff is inactive but TestB still reads {}: the flip changed the fold without dirtying \
         the target, so nothing resolved it",
        test_b.current_value()
    );
}
