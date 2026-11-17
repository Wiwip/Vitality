use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use vitality::actors::ActorBuilder;
use vitality::context::Vitality;
use vitality::effect::AppliedEffects;
use vitality::effect::Effect;
use vitality::effect::conditions::IsAttributeWithinBounds;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::registry::effect_registry::EffectToken;
use vitality::registry::{Registry, RegistryMut};
use vitality::{AttributesPlugin, attribute, init_attribute};

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
    // The attribute shouldn't change till we update
    assert_eq!(test_c.current_value(), init_value);

    app.update();

    let mut query = app.world_mut().query::<&TestInt>();
    let test_c = query.single(app.world()).unwrap();
    assert_eq!(test_c.current_value(), init_value + modifier_value);
}
