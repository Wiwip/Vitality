use crate::actors::{Actor, ActorBuilder};
use crate::assets::EffectDef;
use crate::context::Vitality;
use bevy::prelude::*;

/// A plugin that manages global effects within the game.
/// Its purpose is to add effects that are applied to any spawned actor.
pub struct GlobalEffectPlugin;

impl Plugin for GlobalEffectPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, spawn_global_actor);
        app.insert_resource(GlobalEffects(vec![]));

        app.add_observer(observe_spawned_actor);
    }
}

#[derive(Resource, Deref, DerefMut)]
pub struct GlobalEffects(Vec<Handle<EffectDef>>);

#[derive(Component, Clone, Copy)]
pub struct GlobalActor;

#[derive(Component, Clone, Copy)]
pub struct GlobalEffect;

pub fn spawn_global_actor(mut commands: Commands, mut ctx: Vitality) {
    let global_actor = commands.spawn(GlobalActor).id();

    let actor = ActorBuilder::new().name("Global Effect Actor").build();

    let actor_handle = ctx.add_actor(actor);
    ctx.insert_actor(global_actor, &actor_handle);
}

/// When an actor is spawned, ensures that the global effects apply to the actor.
fn observe_spawned_actor(trigger: On<Add, Actor>, mut ctx: Vitality) {
    ctx.spawn_global_effects(trigger.entity);
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::ability::AbilityBuilder;
    use crate::actors::{Actor, ActorBuilder};
    use crate::assets::AbilityDef;
    use crate::condition::IsAttributeWithinBounds;
    use crate::context::Vitality;
    use crate::effect::{Effect, EffectInactive};
    use crate::modifier::{EffectSubject, ModOp};
    use crate::prelude::*;
    use crate::registry::ability_registry::AbilityToken;
    use crate::registry::effect_registry::EffectToken;
    use crate::registry::{Registry, RegistryMut};
    use crate::{AttributesPlugin, attribute, init_attribute};
    use bevy::ecs::system::RunSystemOnce;

    attribute!(TestAttribute, f64);

    #[derive(Component, Copy, Clone, Debug, PartialEq)]
    struct ConditionTag;

    fn prepare_actor(mut ctx: Vitality, registry: Registry) {
        let actor_template = ctx.add_actor(
            ActorBuilder::new()
                .name("TestActor".into())
                .with::<TestAttribute>(0.0)
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
                .modify::<TestAttribute>(200.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );

        registry.add_effect(
            CONDITION_EFFECT,
            Effect::permanent()
                .name("Condition Effect".into())
                .active_while(IsAttributeWithinBounds::<TestAttribute>::target(150.0..))
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
            .build()
    }

    #[test]
    fn test_attribute_condition() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
        app.add_plugins(init_attribute::<TestAttribute>);

        app.add_systems(
            Startup,
            (prepare_effects, prepare_abilities, prepare_actor).chain(),
        );

        app.update();

        let mut query = app.world_mut().query::<(Entity, &Actor, &TestAttribute)>();
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
}
