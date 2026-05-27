use crate::AttributesMut;
use crate::context::EffectExprContext;
use crate::inspector::pretty_type_name;
use crate::math::AbsDiff;
use crate::modifier::calculator::AttributeCalculator;
use crate::prelude::*;
use crate::systems::MarkNodeDirty;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use bevy::reflect::TypeRegistryArc;

#[derive(Message)]
pub struct ApplyAttributeModifierMessage<T: Attribute> {
    pub source_entity: Entity,
    pub target_entity: Entity,
    pub effect_entity: Entity,
    pub modifier: AttributeModifier<T>,
}

pub fn apply_modifier_events<T: Attribute>(
    mut event_reader: MessageReader<ApplyAttributeModifierMessage<T>>,
    mut attributes: Query<AttributesMut, Without<IsResource>>,
    mut commands: Commands,
    type_registry: Res<AppTypeRegistry>,
) {
    for ev in event_reader.read() {
        let has_changed =
            apply_modifier(&ev, &mut attributes, type_registry.0.clone()).unwrap_or(false);

        if has_changed {
            commands.trigger(MarkNodeDirty::<T> {
                entity: ev.target_entity,
                phantom_data: Default::default(),
            });
        }
    }
}

pub fn apply_modifier<T: Attribute>(
    trigger: &ApplyAttributeModifierMessage<T>,
    attributes: &mut Query<AttributesMut, Without<IsResource>>,
    type_registry: TypeRegistryArc,
) -> Result<bool, BevyError> {
    let query = [trigger.source_entity, trigger.target_entity];
    let [source, target] = attributes.get_many(query)?;

    let base_value = target
        .get::<T>()
        .ok_or(format!(
            "Could not find attribute {} on entity {}.",
            pretty_type_name::<T>(),
            trigger.target_entity
        ))?
        .base_value();

    // We update the modifier's internal value before applying it.
    let context = EffectExprContext {
        source_actor: &source,
        target_actor: &target, // Needs to be fixed.
        effect_holder: &source,
        type_registry: type_registry.clone(),
    };
    let mut modifier = trigger.modifier.clone();
    modifier.update_value(&context);

    // Apply the modifier
    let Ok(calculator) = AttributeCalculator::<T>::convert(&modifier) else {
        return Err(format!("Could not convert modifier {} to calculator.", modifier,).into());
    };
    let new_base_value = calculator.eval(base_value);

    let has_changed = new_base_value.are_different(base_value);
    if has_changed {
        let mut attributes_mut = attributes.get_mut(trigger.target_entity)?;

        let mut attribute = attributes_mut.get_mut::<T>().ok_or(format!(
            "Could not find attribute {} on entity {}.",
            pretty_type_name::<T>(),
            trigger.target_entity
        ))?;

        attribute.set_base_value(new_base_value);
    }
    Ok(has_changed)
}
