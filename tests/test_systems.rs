use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use vitality::actors::ActorBuilder;
use vitality::context::Vitality;
use vitality::effect::{Effect, EffectApplicationPolicy, EffectBuilder};
use vitality::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::{AttributesPlugin, attribute, init_attribute};

attribute!(TestA, u32);

fn prepare_actor(mut ctx: Vitality) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("TestActor".into())
            .with::<TestA>(0)
            .build(),
    );
    ctx.spawn_actor_from_handle(&actor_template);
}

/// Creates an actor with attribute TestA(u32) with a base value of 0.
/// Adds a permanent effect adding 42 to the value of the attribute.
/// Asserts that the attribute is now 42.
/// Removes the effect and assert that the value is returned to 0.
#[test]
fn test_update_current_values() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(init_attribute::<TestA>);
    app.add_systems(Startup, prepare_actor);

    app.update();

    let mut query = app.world_mut().query::<(Entity, &TestA)>();
    let (entity, _) = query.single(app.world()).unwrap();

    let attribute = app.world().get::<TestA>(entity).unwrap();
    assert_eq!(0, attribute.base_value());
    assert_eq!(0, attribute.current_value());

    // Create a dynamic effect and apply it directly to the entity
    app.world_mut()
        .run_system_once(move |mut ctx: Vitality| {
            let effect_def = EffectBuilder::new(EffectApplicationPolicy::Permanent)
                .modify::<TestA>(42u32, ModOp::Add, EffectSubject::Target)
                .build();

            ctx.apply_dynamic_effect_to_self(entity, effect_def);
        })
        .unwrap();

    app.update();

    let attribute = app.world().get::<TestA>(entity).unwrap();
    assert_eq!(0, attribute.base_value());
    assert_eq!(42, attribute.current_value());

    // Despawn the effect and confirm the attribute values
    let mut query = app.world_mut().query::<(Entity, &Effect)>();
    let (effect_entity, _) = query.single(app.world()).unwrap();
    app.world_mut().despawn(effect_entity);

    app.update();

    let attribute = app.world().get::<TestA>(entity).unwrap();
    assert_eq!(0, attribute.base_value());
    assert_eq!(0, attribute.current_value());
}
