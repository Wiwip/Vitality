use crate::context::{EffectExprContext, EffectExprContextMut, EffectExprSchema};
use crate::effect::{EffectSource, EffectTarget};
use crate::inspector::pretty_type_name;
use crate::math::AbsDiff;
use crate::modifier::calculator::ModOp;
use crate::modifier::{
    ApplyAttributeModifierMessage, AttributeCalculator, ModifierMarker, ModifierOf,
};
use crate::modifier::{EffectSubject, ReflectAccessModifier};
use crate::prelude::*;
use crate::systems::MarkNodeDirty;
use crate::{AttributeBindings, AttributesRef};
use bevy::prelude::*;
use bevy::reflect::TypeRegistryArc;
use smol_str::SmolStr;
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;

pub trait Modifier: Send + Sync {
    /// Spawns the modifier as a component on the effect, targeting the actor for observers.
    /// The EntityCommand is the already inserted attribute modifier component.
    fn spawn_persistent_modifier(
        &self,
        actor_entity: Entity,
        ctx: &EffectExprContext,
        type_bindings: &AttributeBindings,
        commands: &mut EntityCommands,
    );

    /// Immediately makes the modifications to the attributes.
    /// Good for ability cost calculations. Prevents them from paying the cost once but doubly activate.
    fn apply_immediate(&self, context: &mut EffectExprContextMut) -> bool;

    /// Sends a message to apply the message at the end of the schedule together with all other mods.
    /// Good for damage, heals, etc.
    fn apply_delayed(
        &self,
        source: Entity,
        target: Entity,
        effect: Entity,
        commands: &mut Commands,
    );
}

#[derive(Component, Clone, Reflect)]
#[reflect(Component, from_reflect = false)]
#[reflect(AccessModifier)]
#[require(ModifierMarker)]
pub struct AttributeModifier<T: Attribute> {
    #[reflect(ignore)]
    pub expr: Arc<dyn Expr<T::Property, EffectExprSchema> + Send + Sync>,
    pub value: T::Property,
    pub who: EffectSubject,
    pub operation: ModOp,
}

impl<T> AttributeModifier<T>
where
    T: Attribute + 'static,
{
    pub fn new(
        value: T::Property,
        modifier: ModOp,
        who: EffectSubject,
        expr: Arc<dyn Expr<T::Property, EffectExprSchema> + Send + Sync>,
    ) -> Self {
        Self {
            expr,
            value,
            who,
            operation: modifier,
        }
    }

    pub fn update_value(&mut self, ctx: &EffectExprContext) {
        let new_val = self.expr.eval(ctx);
        self.value = new_val;
    }
}

impl<T> Modifier for AttributeModifier<T>
where
    T: Attribute,
{
    fn spawn_persistent_modifier(
        &self,
        actor_entity: Entity,
        ctx: &EffectExprContext,
        type_bindings: &AttributeBindings,
        commands: &mut EntityCommands,
    ) {
        let value = self.expr.eval(ctx);

        let modifier = AttributeModifier::<T> {
            expr: self.expr.clone(),
            value,
            who: self.who,
            operation: self.operation,
        };
        let display = modifier.to_string();

        // Spawn the observer. Watches the actor for attribute value changes.
        let mut deps = HashSet::default();
        self.expr.get_dependencies(&mut deps);
        for dep in deps {
            let Some(attr_dep) = type_bindings.insert_dependency_functions.get(&dep) else {
                error!(
                    "Expression dependency {:?} is not a registered attribute dependency for modifier {}",
                    dep,
                    pretty_type_name::<T>(),
                );
                continue;
            };
            attr_dep(actor_entity, commands);
        }

        commands.insert((modifier, Name::new(format!("{}", display))));
    }
    fn apply_immediate(&self, context: &mut EffectExprContextMut) -> bool {
        let immutable_context = EffectExprContext {
            source_actor: context.source_actor.as_readonly(),
            target_actor: context.target_actor.as_ref().map(|v| v.as_readonly()),
            effect_holder: context.effect_holder.as_readonly(),
        };

        let calc = AttributeCalculator::<T>::convert(self);

        let attr_ref = match self.who {
            EffectSubject::Target => &immutable_context
                .target_actor
                .as_ref()
                .unwrap_or(&immutable_context.source_actor),
            EffectSubject::Source => &immutable_context.source_actor,
            EffectSubject::Effect => &immutable_context.effect_holder,
        };
        let Some(attribute) = attr_ref.get::<T>() else {
            return false;
        };
        let new_val = calc.eval(attribute.base_value());

        let attributes_mut = match self.who {
            EffectSubject::Target => &mut context.target_actor.as_mut().unwrap_or(&mut context.source_actor),
            EffectSubject::Source => &mut context.source_actor,
            EffectSubject::Effect => &mut context.effect_holder,
        };
        // Apply the modifier
        if let Some(mut attribute) = attributes_mut.get_mut::<T>() {
            // Ensure that the modifier meaningfully changed the value before we trigger the event.
            let has_changed = new_val.are_different(attribute.current_value());
            if has_changed {
                attribute.set_base_value(new_val);
            }
            has_changed
        } else {
            panic!("Could not find attribute {}", pretty_type_name::<T>());
        }
    }

    fn apply_delayed(
        &self,
        source: Entity,
        target: Entity,
        effect: Entity,
        commands: &mut Commands,
    ) {
        commands.write_message(ApplyAttributeModifierMessage::<T> {
            source_entity: source,
            target_entity: target,
            effect_entity: effect,
            modifier: self.clone(),
        });
    }
}

impl<T> Display for AttributeModifier<T>
where
    T: Attribute,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Mod<{}>({}{}) {}",
            pretty_type_name::<T>(),
            self.operation,
            self.value,
            self.who,
        )
    }
}

#[derive(EntityEvent)]
pub struct RecalculateExpression {
    #[event_target]
    pub modifier_entity: Entity,
}

/// When the attribute changes, update the values of dependent AttributeModifier<T>.
pub fn update_modifier_when_dependencies_changed<T: Attribute>(
    trigger: On<RecalculateExpression>,
    mut modifiers: Query<(&mut AttributeModifier<T>, &ModifierOf)>,
    effects: Query<(&EffectSource, &EffectTarget)>,
    actors: Query<AttributesRef, Without<AttributeModifier<T>>>,
    mut commands: Commands,
) {
    let Ok((mut modifier, effect_id)) = modifiers.get_mut(trigger.modifier_entity) else {
        return;
    };
    let (source, target) = effects.get(effect_id.0).unwrap();
    let [source_ref, target_ref] = actors.get_many([source.0, target.0]).unwrap();

    let context = EffectExprContext {
        target_actor: Some(target_ref),
        source_actor: source_ref,
        effect_holder: source_ref,
    };

    let new_val = modifier.expr.eval(&context);
    modifier.value = new_val;

    commands.trigger(MarkNodeDirty::<T> {
        entity: effect_id.0,
        phantom_data: Default::default(),
    });
}
