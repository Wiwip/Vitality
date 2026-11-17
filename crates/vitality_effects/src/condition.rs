use crate::EffectsRef;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::systems::MarkActorDirty;
use crate::{Effect, EffectInactive, EffectSource, EffectTarget, EffectTicker};
use bevy::asset::Assets;
use bevy::ecs::relationship::Relationship;
use bevy::log::error;
use bevy::prelude::*;
use vitality_core::schedule::EffectsSet;

pub struct ConditionPlugin;

impl Plugin for ConditionPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        app.add_systems(
            FixedUpdate,
            evaluate_effect_conditions.in_set(EffectsSet::Prepare),
        );
    }
}

/// one non-periodic effect with the actors it reads its conditions from
type ConditionRow<'w, 's> = (
    EffectsRef<'w, 's>,
    &'w Effect,
    &'w EffectSource,
    &'w EffectTarget,
    Option<&'w EffectInactive>,
);

/// the row is read-only so this could parallelise; the split costs more single-threaded than it wins threaded
fn evaluate_effect_conditions(
    mut query: Query<ConditionRow<'_, '_>, Without<EffectTicker>>,
    parents: Query<EffectsRef>,
    effects: Res<Assets<EffectDef>>,
    mut commands: Commands,
) {
    for (effect_entity_ref, effect, source, target, status) in query.iter_mut() {
        let effect_entity = effect_entity_ref.id();
        let Some(effect) = effects.get(&effect.0) else {
            error!(
                "Effect {} has no effect definition.",
                effect_entity_ref.id()
            );
            continue;
        };

        // an empty list is always active and this system is the only writer of EffectInactive, so a row
        // without a marker has nothing to decide
        if effect.activate_conditions.is_empty() && status.is_none() {
            continue;
        }

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

        let context = EffectExprContext {
            target_actor: Some(target_actor_ref),
            source_actor: source_actor_ref,
            effect_holder: effect_entity_ref,
        };

        let should_be_active = effect
            .activate_conditions
            .iter()
            .all(|condition| condition.eval(&context));

        let is_inactive = status.is_some();
        if should_be_active && is_inactive {
            debug!("Effect {effect_entity} is now active.");
            commands.entity(effect_entity).remove::<EffectInactive>();
            // nothing else dirties the target: no attribute of its own changed and no leftover mark carries the fold into the next tick
            commands.trigger(MarkActorDirty { entity: target.0 });
        } else if !should_be_active && !is_inactive {
            debug!("Effect {effect_entity} is now inactive.");
            commands.entity(effect_entity).insert(EffectInactive);
            commands.trigger(MarkActorDirty { entity: target.0 });
        }
    }
}
