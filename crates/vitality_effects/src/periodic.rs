use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use vitality_core::AttributesRef;
use vitality_core::assets::EffectDef;
use vitality_core::attributes::Attribute;
use vitality_core::context::EffectExprContext;
use vitality_core::effect::{Effect, EffectSource, EffectTarget, EffectTicker};
use vitality_core::modifier::{ApplyAttributeModifierMessage, AttributeModifier, OwnedModifiers};

pub(crate) fn apply_periodic_effect<T: Attribute>(
    actors: Query<AttributesRef, Without<IsResource>>,
    effects: Query<
        (
            AttributesRef,
            &Effect,
            &EffectTicker,
            &OwnedModifiers,
            &EffectTarget,
            &EffectSource,
        ),
        Without<IsResource>,
    >,
    modifiers: Query<&AttributeModifier<T>>,
    mut event_writer: MessageWriter<ApplyAttributeModifierMessage<T>>,
    effect_assets: Res<Assets<EffectDef>>,
) {
    for (effect_ref, effect, timer, owned_modifiers, target, source) in effects.iter() {
        if !timer.just_finished() {
            continue;
        }

        let effect_def = effect_assets
            .get(&effect.0)
            .ok_or("No effect asset.")
            .unwrap();

        let source_actor_ref = actors.get(source.0).unwrap();
        let target_actor_ref = actors.get(target.0).unwrap();

        let context = EffectExprContext {
            target_actor: Some(target_actor_ref),
            source_actor: source_actor_ref,
            effect_holder: effect_ref,
        };

        // Determines whether the effect should activate
        let should_be_activated = effect_def
            .activate_conditions
            .iter()
            .all(|condition| condition.eval(&context));

        if !should_be_activated {
            continue;
        }

        // Timer has triggered. Grab modifiers and apply them.
        for children in owned_modifiers.iter() {
            let Ok(attribute_modifier) = modifiers.get(children) else {
                continue;
            };

            // Clone the modifier so we can apply the stack count to it.
            let applied_modifier = attribute_modifier.clone();

            event_writer.write(ApplyAttributeModifierMessage {
                source_entity: source.0,
                target_entity: target.0,
                effect_entity: effect_ref.id(),
                modifier: applied_modifier,
            });
        }
    }
}
