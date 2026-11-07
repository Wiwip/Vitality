use crate::attributes::{Attribute, AttributeDependency, ReflectAccessAttribute};
use crate::modifier::AttributeCalculatorCached;
use crate::schedule::EffectsSet;
use crate::util::pretty_type_name;
use bevy::platform::collections::HashMap;
use bevy::platform::collections::hash_map::Entry;
use bevy::prelude::*;
use bevy::scene::ScenePlugin;
use smol_str::SmolStr;
use std::any::TypeId;
use std::error::Error;
use std::fmt::Formatter;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

pub mod attributes;
pub mod context;
pub mod math;
pub mod modifier;
pub mod mutator;
pub mod schedule;
pub mod util;
pub mod view;

pub use express_it;
pub use num_traits;

pub mod prelude {
    pub use crate::attribute;
    pub use crate::attributes::{AccessAttribute, Attribute, ReflectAccessAttribute};
    pub use crate::context::ActorExprSchema;
    pub use crate::modifier::ModOp;

    pub use express_it::expr::Expr;

    // Necessary for attribute macro
    pub use bevy::prelude::ReflectComponent;
}

/// Registers the schedule sets and resources every other vitality crate builds on.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AppAttributeBindings>()
            .init_schedule(PreUpdate)
            .init_schedule(PostUpdate);

        // ScenePlugin is unique and DefaultPlugins already adds it, so adding it again would panic.
        if !app.is_plugin_added::<ScenePlugin>() {
            app.add_plugins(ScenePlugin);
        }

        app.configure_sets(
            FixedUpdate,
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

#[derive(Resource, Clone, Default)]
pub struct AppAttributeBindings {
    pub internal: Arc<RwLock<AttributeBindings>>,
}

#[derive(Default)]
pub struct AttributeBindings {
    type_id_map: HashMap<SmolStr, TypeId>,
    insert_dependency_functions: HashMap<TypeId, fn(Entity, &mut EntityCommands)>,
}

impl AttributeBindings {
    fn add<T: Attribute>(&mut self) {
        self.bind_type_id::<T>();

        self.insert_dependency_functions
            .insert(TypeId::of::<T>(), Self::dependency_fn::<T>);
    }

    /// The function that makes `entity` a dependency of the attribute with this type id.
    pub fn dependency_inserter(&self, type_id: &TypeId) -> Option<fn(Entity, &mut EntityCommands)> {
        self.insert_dependency_functions.get(type_id).copied()
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

    // Inserts dependency injection closures
    fn dependency_fn<T: Attribute>(entity: Entity, commands: &mut EntityCommands) {
        commands.insert(AttributeDependency::<T>::new(entity));
    }
}

/// Registers the reflection data and expression bindings of an attribute.
pub fn init_core_attribute<T: Attribute>(app: &mut App) {
    app.register_type::<T>();
    app.register_type::<AttributeCalculatorCached<T>>();
    app.register_type_data::<T, ReflectAccessAttribute>();

    // Register u64->TypeId bindings for expressions
    app.world_mut()
        .resource_mut::<AppAttributeBindings>()
        .internal
        .write()
        .unwrap()
        .add::<T>();

    debug!("Registered attribute: {}.", pretty_type_name::<T>());
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
