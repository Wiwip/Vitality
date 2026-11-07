use crate::EffectsRef;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::{Effect, EffectInactive, EffectSource, EffectTarget, EffectTicker};
use bevy::asset::Assets;
use bevy::ecs::relationship::Relationship;
use bevy::log::error;
use bevy::prelude::*;
use vitality_core::schedule::EffectsSet;

pub struct ConditionPlugin;

impl Plugin for ConditionPlugin {
    fn build(&self, app: &mut App) {
        // activates/deactivates effects based on their conditions
        app.add_systems(
            Update,
            evaluate_effect_conditions.in_set(EffectsSet::Prepare),
        );
    }
}

pub fn evaluate_effect_conditions(
    mut query: Query<
        (
            EffectsRef,
            &Effect,
            &EffectSource,
            &EffectTarget,
            Option<&EffectInactive>,
        ),
        Without<EffectTicker>,
    >,
    parents: Query<EffectsRef>,
    effects: Res<Assets<EffectDef>>,
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
            target_actor: Some(target_actor_ref),
            source_actor: source_actor_ref,
            effect_holder: effect_entity_ref,
        };

        // Determines whether the effect should activate
        let should_be_active = effect
            .activate_conditions
            .iter()
            .all(|condition| condition.eval(&context));

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
