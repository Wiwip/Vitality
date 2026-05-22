extern crate core;

use crate::effect::{AttributeDependency, Stacks};
use bevy::prelude::*;
use std::any::{Any, TypeId};
use std::error::Error;
use std::fmt::Formatter;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

pub mod ability;
pub mod actors;
pub mod assets;
mod attribute;
pub mod attributes;
pub mod condition;
pub mod context;
pub mod effect;
pub mod graph;
pub mod inspector;
pub mod math;
pub mod modifier;
pub mod mutator;
pub mod registry;
mod schedule;
mod systems;
mod trigger;

use crate::ability::{Ability, AbilityCooldown, AbilityOf, AbilityPlugin, GrantedAbilities};
use crate::assets::{AbilityDef, ActorDef, EffectDef};
use crate::attributes::{
    on_add_attribute, on_change_notify_attribute_dependencies, on_change_notify_attribute_parents,
    ReflectAccessAttribute,
};
use crate::condition::ConditionPlugin;
use crate::effect::global_effect::GlobalEffectPlugin;
use crate::effect::{
    AppliedEffects, Effect, EffectDuration, EffectSource, EffectSources, EffectTarget,
    EffectTicker, EffectsPlugin,
};
use crate::graph::NodeType;
use crate::inspector::pretty_type_name;
use crate::modifier::{
    apply_modifier_events, ApplyAttributeModifierMessage, AttributeCalculatorCached, ModifierOf,
};
use crate::prelude::*;
use crate::registry::RegistryPlugin;
use crate::schedule::EffectsSet;
use crate::systems::{
    apply_periodic_effect, mark_node_dirty_observer, update_attribute, update_current_value_system,
};
use bevy::ecs::world::{EntityMutExcept, EntityRefExcept};
use bevy::platform::collections::hash_map::Entry;
use bevy::platform::collections::HashMap;

pub mod prelude {
    pub use crate::attribute;
    pub use crate::attributes::{
        AccessAttribute, Attribute, AttributeTypeId, ReflectAccessAttribute,
    };
    pub use crate::context::{AbilityExprSchema, ActorExprSchema, EffectExprSchema};
    pub use crate::effect::{EffectApplicationPolicy, EffectBuilder};
    pub use crate::modifier::{AccessModifier, AttributeModifier, EffectSubject, ModOp};

    pub use express_it::expr::ExprSchema;

    // Necessary for attribute macro
    pub use bevy::prelude::ReflectComponent;
}

use crate::attribute::clamps::{apply_clamps, update_clamps, Clamp};
use crate::modifier::modifier::update_modifier_when_dependencies_changed;

pub use express_it;
pub use num_traits;
use smol_str::SmolStr;

pub struct AttributesPlugin;

impl Plugin for AttributesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AppAttributeBindings>()
            .add_plugins((
                AbilityPlugin,
                ConditionPlugin,
                EffectsPlugin,
                GlobalEffectPlugin,
                RegistryPlugin,
            ))
            .add_plugins(init_attribute::<Stacks>)
            .init_schedule(PreUpdate)
            .init_schedule(PostUpdate)
            .init_asset::<ActorDef>()
            .init_asset::<EffectDef>()
            .init_asset::<AbilityDef>()
            .register_type::<AppliedEffects>()
            .register_type::<EffectTarget>()
            .register_type::<NodeType>();

        app.configure_sets(
            Update,
            (
                EffectsSet::First,
                EffectsSet::Prepare,
                EffectsSet::UpdateBaseValues,
                EffectsSet::UpdateCurrentValues,
                EffectsSet::Notify,
                EffectsSet::Last,
            )
                .chain(),
        );
    }
}

impl AttributesPlugin {
    pub fn default() -> Self {
        Self
    }
}

#[derive(Resource, Clone, Default)]
pub struct AppAttributeBindings {
    pub internal: Arc<RwLock<AttributeBindings>>,
}

#[derive(Default)]
pub struct AttributeBindings {
    type_id_map: HashMap<SmolStr, TypeId>,
    convert: HashMap<SmolStr, fn(&dyn Any) -> Option<&dyn Reflect>>,
    how_to_insert_dependency: HashMap<SmolStr, fn(Entity, &mut EntityCommands)>,
}

impl AttributeBindings {
    fn add<T: Attribute>(&mut self) {
        let name = pretty_type_name::<T>();

        self.bind_type_id::<T>();

        self.convert.insert(name.clone().into(), Self::convert_fn::<T>);

        self.how_to_insert_dependency
            .insert(name.clone().into(), Self::dependency_fn::<T>);
    }

    // Binds the AttributeId to a specific TypeId used for reflection
    fn bind_type_id<T: 'static>(&mut self) {
        let type_id = TypeId::of::<T>();
        let name = pretty_type_name::<T>();

        match self.type_id_map.entry(SmolStr::new(name.clone())) {
            Entry::Vacant(e) => {
                trace!("{}: {}", pretty_type_name::<T>(), name.clone());
                e.insert(type_id);
            }
            Entry::Occupied(_) => {
                panic!(
                    "Attribute type ID collision for {} (id: {:?}). Was the attribute registered twice?",
                    pretty_type_name::<T>(),
                    type_id,
                );
            }
        };
    }

    // Allows conversions from dyn Any to dyn Reflect when all we know is the attribute ID
    fn convert_fn<T: Attribute>(any: &dyn Any) -> Option<&dyn Reflect> {
        any.downcast_ref::<T::Property>()
            .map(|value| value.as_reflect())
    }

    // Inserts dependency injection closures
    fn dependency_fn<T: Attribute>(entity: Entity, commands: &mut EntityCommands) {
        commands.insert(AttributeDependency::<T>::new(entity));
    }
}

pub fn init_attribute<T: Attribute>(app: &mut App) {
    app.register_type::<T>();
    app.register_type::<AttributeModifier<T>>();
    app.register_type::<Clamp<T>>();
    app.register_type::<AttributeCalculatorCached<T>>();
    app.register_type_data::<T, ReflectAccessAttribute>();
    app.add_message::<ApplyAttributeModifierMessage<T>>();

    // Register u64->TypeId bindings for expressions
    app.world_mut()
        .resource_mut::<AppAttributeBindings>()
        .internal
        .write()
        .unwrap()
        .add::<T>();

    app.add_systems(
        Update,
        apply_periodic_effect::<T>.in_set(EffectsSet::Prepare),
    );

    app.add_systems(
        Update,
        apply_modifier_events::<T>.in_set(EffectsSet::UpdateBaseValues),
    );

    app.add_systems(
        Update,
        (update_current_value_system::<T>, apply_clamps::<T>)
            .chain()
            .in_set(EffectsSet::UpdateCurrentValues),
    );

    app.add_systems(
        Update,
        (
            on_change_notify_attribute_parents::<T>.in_set(EffectsSet::Notify),
            on_change_notify_attribute_dependencies::<T>.in_set(EffectsSet::Notify),
        ),
    );

    app.add_observer(mark_node_dirty_observer::<T>);
    app.add_observer(on_add_attribute::<T>);
    app.add_observer(update_attribute::<T>);
    app.add_observer(update_modifier_when_dependencies_changed::<T>);
    app.add_observer(update_clamps::<T>);

    debug!(
        "Registered Systems for attribute: {}.",
        pretty_type_name::<T>()
    );
}

pub type AttributesMut<'w, 's> = EntityMutExcept<
    'w,
    's,
    (
        // We exclude anything related to effects
        ChildOf,
        Effect,
        EffectDuration,
        EffectTicker,
        EffectSource,
        EffectTarget,
        AppliedEffects,
        EffectSources,
        Ability,
        GrantedAbilities,
        AbilityOf,
        AbilityCooldown,
        ModifierOf,
    ),
>;

pub type AttributesRef<'w, 's> = EntityRefExcept<
    'w,
    's,
    (
        // We exclude anything related to effects
        ChildOf,
        Effect,
        EffectDuration,
        EffectTicker,
        EffectSource,
        EffectTarget,
        AppliedEffects,
        EffectSources,
        Ability,
        GrantedAbilities,
        AbilityOf,
        AbilityCooldown,
        ModifierOf,
    ),
>;

#[derive(Component, Copy, Clone, Debug)]
#[component(storage = "SparseSet")]
pub struct Dirty<T> {
    phantom: PhantomData<T>,
}

impl<T> Default for Dirty<T> {
    fn default() -> Self {
        Self {
            phantom: PhantomData,
        }
    }
}

#[derive(EntityEvent, Debug)]
pub struct BaseValueChanged<T: Attribute> {
    pub phantom_data: PhantomData<T>,
    pub old: T::Property,
    pub new: T::Property,
    pub entity: Entity,
}

#[derive(EntityEvent, Debug)]
pub struct CurrentValueChanged<T: Attribute> {
    pub phantom_data: PhantomData<T>,
    pub old: T::Property,
    pub new: T::Property,
    pub entity: Entity,
}

#[derive(Clone, Debug)]
pub enum AttributeError {
    AttributeNotPresent(TypeId),
    PhantomQuery,
}

impl std::fmt::Display for AttributeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributeError::AttributeNotPresent(type_id) => {
                write!(
                    f,
                    "Attribute with TypeId {:?} not present on entity.",
                    type_id
                )
            }
            AttributeError::PhantomQuery => {
                write!(f, "Phantom query on entity does not exist.")
            }
        }
    }
}

impl Error for AttributeError {}
