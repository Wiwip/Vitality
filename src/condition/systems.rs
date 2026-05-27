use crate::AttributesRef;
use crate::assets::EffectDef;
use crate::context::EffectExprContext;
use crate::effect::{Effect, EffectInactive, EffectSource, EffectTarget, EffectTicker};
use bevy::asset::Assets;
use bevy::ecs::relationship::Relationship;
use bevy::log::error;
use bevy::prelude::*;
use express_it::expr::ExprNode;

pub fn evaluate_effect_conditions(
    mut query: Query<
        (
            AttributesRef,
            &Effect,
            &EffectSource,
            &EffectTarget,
            Option<&EffectInactive>,
        ),
        Without<EffectTicker>,
    >,
    parents: Query<AttributesRef>,
    effects: Res<Assets<EffectDef>>,
    type_registry: Res<AppTypeRegistry>,
    mut commands: Commands,
) {
    for (effect_entity_ref, effect, source, target, status) in query.iter_mut() {
        let effect_entity = effect_entity_ref.id();
        let Ok(source_actor_ref) = parents.get(source.get()) else {
            error!(
                "Effect {} has no parent entity {}.",
                effect_entity_ref.id(),
                source.get()
            );
            continue;
        };
        let Ok(target_actor_ref) = parents.get(target.0) else {
            error!(
                "Effect {} has no target entity {}.",
                effect_entity_ref.id(),
                target.get()
            );
            continue;
        };

        let Some(effect) = effects.get(&effect.0) else {
            error!(
                "Effect {} has no effect definition.",
                effect_entity_ref.id()
            );
            continue;
        };

        let context = EffectExprContext {
            target_actor: &target_actor_ref,
            source_actor: &source_actor_ref,
            effect_holder: &effect_entity_ref,
            type_registry: type_registry.0.clone(),
        };

        // Determines whether the effect should activate
        let should_be_active = effect.activate_conditions.iter().all(|condition| {
            condition.inner.eval(&context).unwrap_or_else(|_| {
                error!("A condition failed to execute.");
                false
            })
        });

        let is_inactive = status.is_some();
        if should_be_active && is_inactive {
            // Effect was inactive and its conditions are now met, so activate it.
            debug!("Effect {effect_entity} is now active.");
            commands.entity(effect_entity).remove::<EffectInactive>();
        } else if !should_be_active && !is_inactive {
            // Effect was active and its conditions are no longer met, so deactivate it.
            debug!("Effect {effect_entity} is now inactive.");
            commands.entity(effect_entity).insert(EffectInactive);
        }
    }
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
    use crate::{AttributesPlugin, attribute};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::log::LogPlugin;

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
            .with_cooldown(TestB::src())
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
            crate::init_attribute::<TestA>,
            crate::init_attribute::<TestB>,
        ));

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
}
