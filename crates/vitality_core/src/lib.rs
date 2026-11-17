use crate::attributes::{Attribute, AttributeDependency, ReflectAccessAttribute, dep_key};
use crate::context::{Caster, Source, Target};
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
pub mod registry;
pub mod schedule;
pub mod tags;
pub mod util;
pub mod view;

pub use express_it;
pub use num_traits;

pub mod prelude {
    pub use crate::attribute;
    pub use crate::attributes::{AccessAttribute, Attribute, ReflectAccessAttribute, Setter};
    pub use crate::context::ActorExprSchema;
    pub use crate::modifier::ModOp;

    pub use express_it::expr::Expr;

    // Necessary for attribute macro
    pub use bevy::prelude::ReflectComponent;
}

#[derive(Resource, Default)]
struct InstalledPlugins(bevy::platform::collections::HashSet<TypeId>);

/// true the first time it is called for `P`, so a plugin added twice or after one that already brings it is a no-op instead of a panic
pub fn first_install<P: 'static>(app: &mut App) -> bool {
    app.world_mut()
        .get_resource_or_init::<InstalledPlugins>()
        .0
        .insert(TypeId::of::<P>())
}

/// Registers the schedule sets and resources every other vitality crate builds on.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !crate::first_install::<Self>(app) {
            return;
        }
        app.init_resource::<AppAttributeBindings>()
            .init_resource::<registry::AttributeRegistry>()
            .init_resource::<registry::ResolveQueue>()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DepRole {
    Source,
    Target,
    Caster,
}

#[derive(Clone, Copy)]
pub struct DepBinding {
    pub role: DepRole,
    /// The attribute's own type id, shared by all of its role keys.
    pub attribute: TypeId,
    pub insert: fn(Entity, &mut EntityCommands),
}

#[derive(Default)]
pub struct AttributeBindings {
    type_id_map: HashMap<SmolStr, TypeId>,
    dependencies: HashMap<TypeId, DepBinding>,
}

impl AttributeBindings {
    fn add<T: Attribute>(&mut self) {
        self.bind_type_id::<T>();

        let insert = Self::dependency_fn::<T>;
        // a bare type id predates roles and always meant the source
        for (key, role) in [
            (TypeId::of::<T>(), DepRole::Source),
            (dep_key::<T, Source>(), DepRole::Source),
            (dep_key::<T, Target>(), DepRole::Target),
            (dep_key::<T, Caster>(), DepRole::Caster),
        ] {
            self.dependencies.insert(
                key,
                DepBinding {
                    role,
                    attribute: TypeId::of::<T>(),
                    insert,
                },
            );
        }
    }

    pub fn dependency(&self, type_id: &TypeId) -> Option<DepBinding> {
        self.dependencies.get(type_id).copied()
    }

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
                    "Attribute type ID collision for {} (id: {:?}). Another attribute already holds \
                     the short name: rename one of them. Two crates each shipping a {} is the usual \
                     cause, since the registry keys on the short name and drops the module path; a \
                     deliberate second registration of the same type is the only other way here.",
                    pretty_type_name::<T>(),
                    type_id,
                    pretty_type_name::<T>(),
                );
            }
        };
    }

    fn dependency_fn<T: Attribute>(entity: Entity, commands: &mut EntityCommands) {
        commands.insert(AttributeDependency::<T>::new(entity));
    }
}

/// Registers the reflection data and expression bindings of an attribute.
pub fn init_core_attribute<T: Attribute>(app: &mut App) {
    app.register_type::<T>();
    app.register_type::<AttributeCalculatorCached<T>>();
    app.register_type_data::<T, ReflectAccessAttribute>();

    app.world_mut()
        .resource_scope(|world, mut registry: Mut<registry::AttributeRegistry>| {
            registry.register::<T>(world);
        });

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
