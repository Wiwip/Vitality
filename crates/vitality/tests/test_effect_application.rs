#[path = "support/logs.rs"]
mod logs;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use vitality::actors::ActorBuilder;
use vitality::context::Vitality;
use vitality::effect::AppliedEffects;
use vitality::effect::Effect;
use vitality::effect::EffectApplicationPolicy;
use vitality::effect::conditions::IsAttributeWithinBounds;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::registry::effect_registry::EffectToken;
use vitality::registry::{Registry, RegistryMut};
use vitality::{AttributesPlugin, attribute, init_attribute, tag};
use vitality_core::registry::ResolveQueue;

attribute!(TestA, f32);
attribute!(TestB, f64);
attribute!(TestInt, u32);

fn prepare_actor(mut ctx: Vitality, registry: Registry) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("TestActor")
            .with::<TestA>(100.0)
            .with::<TestB>(10.0)
            .with::<TestInt>(50)
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
            .modify::<TestA>(200.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    registry.add_effect(
        CONDITION_EFFECT,
        Effect::permanent()
            .name("Condition Effect".into())
            .active_while(IsAttributeWithinBounds::<TestA>::target(150.0..))
            .build(),
    );
}

const TEST_EFFECT: EffectToken = EffectToken::new_static("test.test");
const CONDITION_EFFECT: EffectToken = EffectToken::new_static("test.condition");

// both are commands, so an apply in the same system lands behind the spawn it follows
fn spawn_and_apply_in_one_system(mut ctx: Vitality) {
    let actor = ctx
        .add_spawn_actor(
            ActorBuilder::new()
                .name("SameSystemActor")
                .with::<TestInt>(50)
                .build(),
        )
        .id();
    ctx.apply_dynamic_effect_to_self(
        actor,
        EffectBuilder::permanent()
            .modify::<TestInt>(10, ModOp::Add, EffectSubject::Target)
            .build(),
    );
}

fn spawn_actor_only(mut ctx: Vitality) {
    ctx.add_spawn_actor(
        ActorBuilder::new()
            .name("LaterSystemActor")
            .with::<TestInt>(50)
            .build(),
    );
}

fn only_actor(app: &mut App) -> Entity {
    let mut query = app.world_mut().query_filtered::<Entity, With<TestInt>>();
    query.single(app.world()).unwrap()
}

fn applied(app: &App, actor: Entity) -> usize {
    app.world()
        .get::<AppliedEffects>(actor)
        .map_or(0, |applied| applied.iter().count())
}

fn value(app: &App, actor: Entity) -> u32 {
    app.world().get::<TestInt>(actor).unwrap().current_value()
}

#[test]
fn test_apply_in_the_spawn_system_is_applied() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(init_attribute::<TestInt>);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(Startup, spawn_and_apply_in_one_system);

    app.update();
    let actor = only_actor(&mut app);
    app.update();

    assert_eq!(
        applied(&app, actor),
        1,
        "the apply landed on the actor the same system spawned"
    );
    assert_eq!(value(&app, actor), 60);
}

#[test]
fn test_apply_after_the_spawn_system_is_applied() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(init_attribute::<TestInt>);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(Startup, spawn_actor_only);

    app.update();
    let actor = only_actor(&mut app);

    app.world_mut()
        .run_system_once(move |mut ctx: Vitality| {
            ctx.apply_dynamic_effect_to_self(
                actor,
                EffectBuilder::permanent()
                    .modify::<TestInt>(10, ModOp::Add, EffectSubject::Target)
                    .build(),
            );
        })
        .unwrap();

    app.update();

    assert_eq!(applied(&app, actor), 1);
    assert_eq!(value(&app, actor), 60);
}

#[test]
fn test_apply_to_an_entity_with_no_attribute_warns() {
    let logs = logs::Logs::scoped();

    let (bare, effect) = logs.run(|| {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
        app.add_plugins(init_attribute::<TestInt>);
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

        app.update();

        let bare = app.world_mut().spawn(()).id();
        let effect = app
            .world_mut()
            .run_system_once(move |mut ctx: Vitality| {
                ctx.apply_dynamic_effect_to_self(
                    bare,
                    EffectBuilder::permanent()
                        .modify::<TestInt>(10, ModOp::Add, EffectSubject::Target)
                        .build(),
                )
                .id()
            })
            .unwrap();

        app.update();

        (bare, effect)
    });

    let warns = logs.warns();
    assert!(
        warns.contains(&bare.to_string())
            && warns.contains("no registered attribute")
            && warns.contains(&effect.to_string()),
        "an apply to an entity with nothing to fold into has to warn at WARN level and name both \
         the effect and that entity, got: {}",
        logs.all()
    );
}

#[test]
fn test_instant_effect_application() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(init_attribute::<TestInt>);
    // A headless update lasts microseconds, so with the real clock FixedUpdate never runs.
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(Startup, (prepare_effects, prepare_actor).chain());

    app.update();

    let mut query = app.world_mut().query::<(Entity, &TestInt)>();
    let (actor_entity, test_value) = query.single(app.world()).unwrap();
    let init_value = test_value.current_value();

    app.update();

    let modifier_value = 10;
    app.world_mut()
        .run_system_once(move |mut ctx: Vitality| {
            let effect = EffectBuilder::instant()
                .modify::<TestInt>(modifier_value, ModOp::Add, EffectSubject::Target)
                .build();

            ctx.apply_dynamic_effect_to_self(actor_entity, effect);
        })
        .unwrap();

    let mut query = app.world_mut().query::<&TestInt>();
    let test_c = query.single(app.world()).unwrap();
    assert_eq!(test_c.current_value(), init_value);

    app.update();

    let mut query = app.world_mut().query::<&TestInt>();
    let test_c = query.single(app.world()).unwrap();
    assert_eq!(test_c.current_value(), init_value + modifier_value);
}

attribute!(TestU32, u32);

fn prepare_zero_actor(mut ctx: Vitality) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("TestU32ctor")
            .with::<TestU32>(0)
            .build(),
    );
    ctx.spawn_actor_from_handle(&actor_template);
}

#[test]
fn test_update_current_values() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(init_attribute::<TestU32>);
    app.add_systems(Startup, prepare_zero_actor);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.update();

    let mut query = app.world_mut().query::<(Entity, &TestU32)>();
    let (entity, _) = query.single(app.world()).unwrap();

    let attribute = app.world().get::<TestU32>(entity).unwrap();
    assert_eq!(0, attribute.base_value());
    assert_eq!(0, attribute.current_value());

    app.world_mut()
        .run_system_once(move |mut ctx: Vitality| {
            let effect_def = EffectBuilder::new(EffectApplicationPolicy::Permanent)
                .modify::<TestU32>(42u32, ModOp::Add, EffectSubject::Target)
                .build();

            ctx.apply_dynamic_effect_to_self(entity, effect_def);
        })
        .unwrap();

    app.update();

    let attribute = app.world().get::<TestU32>(entity).unwrap();
    assert_eq!(0, attribute.base_value());
    assert_eq!(42, attribute.current_value());

    let mut query = app.world_mut().query::<(Entity, &Effect)>();
    let (effect_entity, _) = query.single(app.world()).unwrap();
    app.world_mut().despawn(effect_entity);

    app.update();

    let attribute = app.world().get::<TestU32>(entity).unwrap();
    assert_eq!(0, attribute.base_value());
    assert_eq!(0, attribute.current_value());
}

tag!(Chilled);

// draining is the only way to read what the next resolve has lined up for `actor`
fn queued_for(app: &App, actor: Entity) -> usize {
    let (pending, dirty) = app.world().resource::<ResolveQueue>().take();
    pending
        .iter()
        .filter(|(entity, _)| *entity == actor)
        .flat_map(|(_, mark)| mark.dirty.iter().chain(mark.regather.iter()))
        .count()
        + dirty.iter().filter(|(entity, _)| *entity == actor).count()
}

#[test]
fn test_an_effect_with_no_modifier_marks_no_attribute() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins((init_attribute::<TestA>, init_attribute::<TestB>));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    let hero = app
        .world_mut()
        .run_system_once(|mut ctx: Vitality| {
            ctx.add_spawn_actor(
                ActorBuilder::new()
                    .name("TwoAttrs")
                    .with::<TestA>(1.0)
                    .with::<TestB>(2.0)
                    .build(),
            )
            .id()
        })
        .unwrap();
    app.update();
    // the marks the spawn itself left are not what this is about
    app.world().resource::<ResolveQueue>().take();

    let handle = app
        .world_mut()
        .run_system_once(move |mut ctx: Vitality| {
            ctx.apply_dynamic_effect_to_self(
                hero,
                EffectBuilder::permanent().with_tag::<Chilled>().build(),
            )
        })
        .unwrap();
    assert!(
        app.world().get::<Chilled>(hero).is_some(),
        "the apply has to land or the mark says nothing"
    );
    assert_eq!(
        queued_for(&app, hero),
        0,
        "an effect carrying no modifier names no attribute to fold"
    );

    let removed = app
        .world_mut()
        .run_system_once(move |mut effects: Effects| effects.remove_by_def(hero, &handle))
        .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(
        queued_for(&app, hero),
        0,
        "removing an effect that never marked an attribute marks none either"
    );
}
