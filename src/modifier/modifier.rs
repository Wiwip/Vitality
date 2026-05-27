use crate::context::{EffectExprContext, EffectExprContextMut, EffectExprSchema, split_path};
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
use express_it::expr::{Expr, ExprNode, SelectExprNodeImpl};
use smol_str::SmolStr;
use std::collections::HashSet;
use std::fmt::Display;

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
    fn apply_immediate(
        &self,
        context: &mut EffectExprContextMut,
        type_registry: TypeRegistryArc,
    ) -> bool;

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
    pub expr: Expr<T::Property, EffectExprSchema>,
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
        expr: Expr<T::Property, EffectExprSchema>,
    ) -> Self {
        Self {
            expr,
            value,
            who,
            operation: modifier,
        }
    }

    pub fn update_value(&mut self, ctx: &EffectExprContext) {
        let new_val = self.expr.inner.eval(ctx).unwrap_or(T::Property::default());
        self.value = new_val;
    }
}

impl<T> Modifier for AttributeModifier<T>
where
    T: Attribute,
    T::Property: SelectExprNodeImpl<EffectExprSchema, Property = T::Property>,
{
    fn spawn_persistent_modifier(
        &self,
        actor_entity: Entity,
        ctx: &EffectExprContext,
        type_bindings: &AttributeBindings,
        commands: &mut EntityCommands,
    ) {
        let value = match self.expr.eval(ctx) {
            Ok(value) => value,
            Err(err) => {
                error!("{}", err);
                return;
            }
        };

        let modifier = AttributeModifier::<T> {
            expr: self.expr.clone(),
            value,
            who: self.who,
            operation: self.operation,
        };
        let display = modifier.to_string();

        // Spawn the observer. Watches the actor for attribute value changes.
        let mut dependencies = HashSet::default();
        self.expr.inner.get_dependencies(&mut dependencies);
        for dependency in dependencies {
            let (_, component, _) = split_path(&dependency.0).expect("Failed to split path");

            let attr_dep = type_bindings
                .how_to_insert_dependency
                .get(&SmolStr::new(component))
                .unwrap();
            attr_dep(actor_entity, commands);
        }

        commands.insert((modifier, Name::new(format!("{}", display))));
    }
    fn apply_immediate(
        &self,
        context: &mut EffectExprContextMut,
        type_registry: TypeRegistryArc,
    ) -> bool {
        let immutable_context = EffectExprContext {
            source_actor: &context.source_actor.as_readonly(),
            target_actor: &context.source_actor.as_readonly(), // Needs to be fixed.
            effect_holder: &context.owner.as_readonly(),
            type_registry: type_registry.clone(),
        };

        let Ok(calc) = AttributeCalculator::<T>::convert(self) else {
            return false;
        };
        let Some(attribute) = immutable_context
            .attribute_ref(EffectSubject::Target)
            .get::<T>()
        else {
            return false;
        };
        let new_val = calc.eval(attribute.base_value());

        let attributes_mut = context.attribute_mut(self.who);
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
    type_registry: Res<AppTypeRegistry>,
    mut commands: Commands,
) {
    let Ok((mut modifier, effect_id)) = modifiers.get_mut(trigger.modifier_entity) else {
        return;
    };
    let (source, target) = effects.get(effect_id.0).unwrap();
    let [source_ref, target_ref] = actors.get_many([source.0, target.0]).unwrap();

    let context = EffectExprContext {
        target_actor: &target_ref,
        source_actor: &source_ref,
        effect_holder: &source_ref,
        type_registry: type_registry.0.clone(),
    };

    let new_val = modifier.expr.eval(&context).unwrap();
    modifier.value = new_val;

    commands.trigger(MarkNodeDirty::<T> {
        entity: effect_id.0,
        phantom_data: Default::default(),
    });
}
