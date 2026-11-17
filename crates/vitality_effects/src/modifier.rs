use crate::EffectTargeting;
use crate::EffectsMut;
use crate::context::{EffectExprContext, EffectExprContextMut, EffectExprSchema};
use crate::prelude::*;
use crate::resolver::ModifierSlot;
use bevy::ecs::message::MessageCursor;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use bevy::prelude::{Component, Entity, EntityCommands, Reflect, reflect_trait};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::any::TypeId;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;
use vitality_core::attributes::record_dependencies;
use vitality_core::math::AbsDiff;
use vitality_core::modifier::AttributeCalculator;
pub use vitality_core::modifier::ModOp;
use vitality_core::num_traits::AsPrimitive;
use vitality_core::util::pretty_type_name;
use vitality_core::{AttributeBindings, DepRole};

pub trait Modifier: Send + Sync {
    /// Spawns the modifier as a component on the effect, targeting the actor for observers.
    /// The EntityCommand is the already inserted attribute modifier component.
    fn spawn_persistent_modifier(
        &self,
        targeting: &EffectTargeting,
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
    pub capture: Capture,
}

/// Whether a persistent modifier follows the attributes its expression reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum Capture {
    /// Re-evaluated whenever a read attribute changes, in the same tick.
    #[default]
    Live,
    /// Evaluated once when the effect is applied.
    Snapshot,
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
            capture: Capture::Live,
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
        targeting: &EffectTargeting,
        ctx: &EffectExprContext,
        type_bindings: &AttributeBindings,
        commands: &mut EntityCommands,
    ) {
        let (value, mut deps) = record_dependencies(|| self.expr.eval(ctx));

        let modifier = AttributeModifier::<T> {
            expr: self.expr.clone(),
            value,
            who: self.who,
            operation: self.operation,
            capture: self.capture,
        };
        let display = modifier.to_string();

        if self.capture == Capture::Live {
            self.expr.get_dependencies(&mut deps);
            let mut seen: Vec<(TypeId, Entity)> = Vec::new();
            for dep in deps {
                let Some(binding) = type_bindings.dependency(&dep) else {
                    error!(
                        "Expression dependency {:?} is not a registered attribute dependency for modifier {}",
                        dep,
                        pretty_type_name::<T>(),
                    );
                    continue;
                };
                let actor = match binding.role {
                    DepRole::Source | DepRole::Caster => targeting.source(),
                    DepRole::Target => targeting.target(),
                };
                if seen.contains(&(binding.attribute, actor)) {
                    continue;
                }
                // one AttributeDependency<A> per entity, so a second actor for A needs a proxy
                if seen
                    .iter()
                    .any(|(attribute, _)| *attribute == binding.attribute)
                {
                    let modifier_entity = commands.id();
                    let mut spawner = commands.commands();
                    let mut proxy = spawner.spawn(DependencyProxyOf(modifier_entity));
                    (binding.insert)(actor, &mut proxy);
                } else {
                    (binding.insert)(actor, commands);
                }
                seen.push((binding.attribute, actor));
            }
        }

        let slot = ModifierSlot {
            attribute: TypeId::of::<T>(),
            op: modifier.operation,
            value: modifier.value.as_(),
        };
        commands.insert((modifier, slot, Name::new(display)));
    }
    fn apply_immediate(&self, context: &mut EffectExprContextMut) -> bool {
        let immutable_context = EffectExprContext {
            source_actor: context.source_actor.as_readonly(),
            target_actor: context.target_actor.as_ref().map(|v| v.as_readonly()),
            effect_holder: context.effect_holder.as_readonly(),
        };

        let calc = AttributeCalculator::<T>::from_op(self.operation, self.value);

        let attr_ref = match self.who {
            EffectSubject::Target => immutable_context
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
            EffectSubject::Target => &mut context
                .target_actor
                .as_mut()
                .unwrap_or(&mut context.source_actor),
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
            warn!("Could not find attribute {}", pretty_type_name::<T>());
            false
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
            stacks: 1,
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

#[derive(Message)]
pub struct ApplyAttributeModifierMessage<T: Attribute> {
    pub source_entity: Entity,
    pub target_entity: Entity,
    pub effect_entity: Entity,
    pub modifier: AttributeModifier<T>,
    pub stacks: u32,
}

/// Drains one attribute's [`ApplyAttributeModifierMessage`] queue; [`ModifierApplies`] holds one per
/// registered attribute so instant modifiers cost a single `FixedUpdate` node for the whole app.
type ModifierApply = fn(&mut World);

#[derive(Resource, Default)]
pub struct ModifierApplies(Vec<(TypeId, ModifierApply)>);

impl ModifierApplies {
    pub fn register<T: Attribute>(&mut self) {
        let attribute = TypeId::of::<T>();
        if !self
            .0
            .iter()
            .any(|(registered, _)| *registered == attribute)
        {
            self.0.push((attribute, apply_queued_modifiers_of::<T>));
        }
    }
}

/// The message cursor and actor query of one attribute, kept as a resource so the collapsed node
/// pays nothing per attribute beyond building them once.
#[derive(Resource)]
struct ApplyState<T: Attribute> {
    cursor: MessageCursor<ApplyAttributeModifierMessage<T>>,
    actors: QueryState<EffectsMut<'static, 'static>, Without<IsResource>>,
}

/// Marks nothing: there is no per-attribute dirty node to mark, and `scan_base_writes` runs after.
fn apply_queued_modifiers_of<T: Attribute>(world: &mut World) {
    if !world.contains_resource::<ApplyState<T>>() {
        let actors = world.query_filtered();
        world.insert_resource(ApplyState::<T> {
            cursor: Default::default(),
            actors,
        });
    }

    // the messages ride out of the world so the actor query can hold it at the same time
    world.resource_scope(
        |world, messages: Mut<Messages<ApplyAttributeModifierMessage<T>>>| {
            world.resource_scope(|world, mut state: Mut<ApplyState<T>>| {
                let state = &mut *state;
                let mut actors = state.actors.query_mut(world);
                for ev in state.cursor.read(&messages) {
                    if let Err(err) = apply_modifier(ev, &mut actors) {
                        debug!("Failed to apply modifier {}. {}", ev.modifier, err);
                    }
                }
            });
        },
    );
}

/// One node for every attribute: asks each registered attribute to apply the modifier messages it
/// queued, so an instant modifier costs one schedule node however many attributes the app has.
pub fn apply_all_modifier_events(world: &mut World, mut applies: Local<Vec<ModifierApply>>) {
    let Some(registry) = world.get_resource::<ModifierApplies>() else {
        return;
    };
    applies.clear();
    applies.extend(registry.0.iter().map(|(_, apply)| *apply));

    for apply in applies.iter() {
        apply(world);
    }
}

pub fn apply_modifier<T: Attribute>(
    trigger: &ApplyAttributeModifierMessage<T>,
    attributes: &mut Query<EffectsMut, Without<IsResource>>,
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
        source_actor: source,
        target_actor: Some(target), // Needs to be fixed.
        effect_holder: source,
    };
    let mut modifier = trigger.modifier.clone();
    modifier.update_value(&context);

    // Apply the modifier
    let calculator = AttributeCalculator::<T>::from_op_stacked(
        modifier.operation,
        modifier.value,
        trigger.stacks,
    );
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

pub type ModifierFn = dyn Fn(&mut EntityCommands, Entity) + Send + Sync;

#[derive(Component, Default, Copy, Clone, Debug, Reflect)]
pub struct ModifierMarker;

#[reflect_trait] // Generates a `ReflectMyTrait` type
pub trait AccessModifier {
    fn describe(&self) -> String;
    fn name(&self) -> String;
}

impl<T> AccessModifier for AttributeModifier<T>
where
    T: Attribute,
{
    fn describe(&self) -> String {
        format!("{}", self)
    }
    fn name(&self) -> String {
        pretty_type_name::<T>()
    }
}

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum EffectSubject {
    Target,
    Source,
    Effect,
}

impl Display for EffectSubject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectSubject::Target => write!(f, "target"),
            EffectSubject::Source => write!(f, "source"),
            EffectSubject::Effect => write!(f, "effect"),
        }
    }
}

impl From<EffectSubject> for SmolStr {
    fn from(value: EffectSubject) -> Self {
        SmolStr::from(value.to_string())
    }
}

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum ActorSubject {
    Actor,
}

impl Display for ActorSubject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorSubject::Actor => write!(f, "actor"),
        }
    }
}

impl From<ActorSubject> for SmolStr {
    fn from(value: ActorSubject) -> Self {
        SmolStr::from(value.to_string())
    }
}

/// Stands in for a modifier that reads one attribute from two actors.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = DependencyProxies)]
pub struct DependencyProxyOf(pub Entity);

#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = DependencyProxyOf, linked_spawn)]
pub struct DependencyProxies(Vec<Entity>);

/// The effect this modifier belongs to.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = OwnedModifiers)]
pub struct ModifierOf(pub Entity);

/// All modifiers belonging to this effect.
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = ModifierOf, linked_spawn)]
pub struct OwnedModifiers(Vec<Entity>);
