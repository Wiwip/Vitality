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
    /// spawns the modifier onto the effect entity; `commands` already carries the attribute modifier component
    fn spawn_persistent_modifier(
        &self,
        targeting: &EffectTargeting,
        ctx: &EffectExprContext,
        type_bindings: &AttributeBindings,
        fires_on_a_timer: bool,
        commands: &mut EntityCommands,
    );

    /// writes the attributes now, so an ability that activates twice cannot pay its cost twice
    fn apply_immediate(&self, context: &mut EffectExprContextMut) -> bool;

    /// queues the write for the collapsed end-of-schedule node instead of touching the attribute now
    fn apply_delayed(
        &self,
        source: Entity,
        target: Entity,
        effect: Entity,
        type_bindings: &AttributeBindings,
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

    fn update_value(&mut self, ctx: &EffectExprContext) {
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
        fires_on_a_timer: bool,
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
        let shown = modifier.to_string();

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
        // the label is the one place a persistent modifier's routing shows, so it says where the write lands
        let label = if fires_on_a_timer || matches!(self.who, EffectSubject::Target) {
            shown
        } else {
            warn_once!(
                "Modifier {shown} names {} but a persistent effect folds every modifier into the \
                 actor that carries it, so this one writes the effect's target",
                self.who,
            );
            format!("{shown}, writes the target")
        };
        commands.insert((modifier, slot, Name::new(label)));
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
        if let Some(mut attribute) = attributes_mut.get_mut::<T>() {
            // Ensure that the modifier meaningfully changed the value before we trigger the event.
            let has_changed = new_val.are_different(attribute.current_value());
            if has_changed {
                attribute.set_base_value(new_val);
            }
            has_changed
        } else {
            warn!(
                "{} not found on the actor this modifier writes: that write is dropped, put the \
                 component on the actor",
                pretty_type_name::<T>()
            );
            false
        }
    }

    fn apply_delayed(
        &self,
        source: Entity,
        target: Entity,
        effect: Entity,
        type_bindings: &AttributeBindings,
        commands: &mut Commands,
    ) {
        // an unregistered attribute owns no Messages<Self>, so the write would die in bevy's command buffer
        if type_bindings.dependency(&TypeId::of::<T>()).is_none() {
            warn_once!(
                "An instant modifier on {} is dropped: that attribute is not registered by this \
                 app, so its write has no queue to ride; register it with init_attribute::<{}>() \
                 in Plugin::build",
                pretty_type_name::<T>(),
                pretty_type_name::<T>(),
            );
            return;
        }

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

/// one attribute's [`ApplyAttributeModifierMessage`] queue; [`ModifierApplies`] holds one per registered attribute so instant modifiers cost a single `FixedUpdate` node
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

/// kept as a resource so the collapsed node pays nothing per attribute beyond building its cursor and query once
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
                        warn!("Deferred modifier {} is dropped: {}", ev.modifier, err);
                    }
                }
            });
        },
    );
}

/// asks each registered attribute to apply the modifier messages it queued, so an instant modifier costs one schedule node however many attributes the app has
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

fn apply_modifier<T: Attribute>(
    trigger: &ApplyAttributeModifierMessage<T>,
    attributes: &mut Query<EffectsMut, Without<IsResource>>,
) -> Result<bool, BevyError> {
    // the write follows `who`; the expression below still reads both actors
    let (writer, other) = match trigger.modifier.who {
        EffectSubject::Target => (trigger.target_entity, trigger.source_entity),
        EffectSubject::Source => (trigger.source_entity, trigger.target_entity),
        EffectSubject::Effect => {
            warn_once!(
                "Modifier {} names EffectSubject::Effect, which is no actor of this effect, so the \
                 write is dropped",
                trigger.modifier
            );
            return Ok(false);
        }
    };
    // the actor the write lands on is the one worth naming first, but a gone bystander drops the
    // write too, because the expression reads it
    if let Some(gone) = [writer, other]
        .into_iter()
        .find(|e| attributes.get(*e).is_err())
    {
        let role = if gone == trigger.source_entity {
            "source"
        } else {
            "target"
        };
        return Err(format!(
            "{role} {gone} has no actor view by the time the write lands, and an instant write \
             lands in the next EffectsSet::UpdateBaseValues, a tick after the apply; keep the \
             {role} alive until then"
        )
        .into());
    }
    let [source, target] = attributes.get_many([trigger.source_entity, trigger.target_entity])?;

    let actor = if writer == trigger.source_entity {
        source
    } else {
        target
    };

    let base_value = actor
        .get::<T>()
        .ok_or(format!(
            "Could not find attribute {} on entity {}.",
            pretty_type_name::<T>(),
            writer
        ))?
        .base_value();

    let context = EffectExprContext {
        source_actor: source,
        target_actor: Some(target), // Needs to be fixed.
        effect_holder: source,
    };
    let mut modifier = trigger.modifier.clone();
    modifier.update_value(&context);

    let calculator = AttributeCalculator::<T>::from_op_stacked(
        modifier.operation,
        modifier.value,
        trigger.stacks,
    );
    let new_base_value = calculator.eval(base_value);

    let has_changed = new_base_value.are_different(base_value);
    if has_changed {
        let mut attributes_mut = attributes.get_mut(writer)?;

        let mut attribute = attributes_mut.get_mut::<T>().ok_or(format!(
            "Could not find attribute {} on entity {}.",
            pretty_type_name::<T>(),
            writer
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

#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = OwnedModifiers)]
pub struct ModifierOf(pub Entity);

#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = ModifierOf, linked_spawn)]
pub struct OwnedModifiers(Vec<Entity>);
