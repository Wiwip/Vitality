use crate::actors::{ActorDef, ActorRegistry};
use crate::clamp::{Clamp, apply_clamps, clamp_dependency_changed};
use bevy::prelude::*;
use vitality_abilities::AbilityPlugin;
use vitality_core::CorePlugin;
use vitality_core::attributes::Attribute;
use vitality_core::init_core_attribute;
use vitality_core::schedule::EffectsSet;
use vitality_effects::{
    ConditionPlugin, EffectsPlugin, GlobalEffectPlugin, Stacks, init_effect_attribute,
};

pub use vitality_abilities as ability;
pub use vitality_core::{
    AppAttributeBindings, AttributeBindings, AttributeError, BaseValueChanged, CurrentValueChanged,
    attribute, attribute_impl, attributes, express_it, math, mutator, num_traits, tag, tags,
};
pub use vitality_effects as effect;

pub mod actors;
pub mod clamp;
pub mod registry;
mod vitality;

pub mod context {
    pub use crate::vitality::Vitality;
    pub use vitality_core::context::*;
}

pub mod prelude {
    pub use crate::context::Vitality;
    pub use crate::{AttributesPlugin, VitalityPlugin, init_attribute};
    pub use vitality_abilities::prelude::*;
    pub use vitality_abilities::{
        Abilities, AbilityActivated, AbilityActivationFailed, AbilityEnded, ActivationError,
    };
    pub use vitality_core::tags::{ActorTags, TagSpec};
    pub use vitality_effects::prelude::*;
    pub use vitality_effects::{
        EffectApplied, EffectBlocked, EffectRemovalReason, EffectRemoved, EffectStacksChanged,
        Effects,
    };
}

#[cfg(feature = "debug-overlay")]
pub mod inspector {
    pub use vitality_core::util::{pretty_type_name, pretty_type_name_str};
    pub use vitality_debug::*;
}

/// Everything vitality needs except the attributes themselves. See [`VitalityPlugin`] for a
/// version that also registers attributes.
#[derive(Default)]
pub struct AttributesPlugin;

impl Plugin for AttributesPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        app.add_plugins((
            CorePlugin,
            AbilityPlugin,
            ConditionPlugin,
            EffectsPlugin,
            GlobalEffectPlugin,
        ))
        .init_asset::<ActorDef>()
        .insert_resource(ActorRegistry::default())
        .add_plugins(init_attribute::<Stacks>);
    }
}

/// [`AttributesPlugin`] plus the attributes this game uses.
///
/// ```ignore
/// app.add_plugins(VitalityPlugin::new().attribute::<Health>().attribute::<Mana>());
/// ```
#[derive(Default)]
pub struct VitalityPlugin {
    attributes: Vec<fn(&mut App)>,
}

impl VitalityPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attribute<T: Attribute>(mut self) -> Self {
        self.attributes.push(init_attribute::<T>);
        self
    }
}

impl Plugin for VitalityPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        app.add_plugins(AttributesPlugin);
        for register in &self.attributes {
            register(app);
        }
    }
}

struct AttributeInstalled<T>(std::marker::PhantomData<T>);

/// Registers one attribute. Safe to call more than once for the same `T`.
pub fn init_attribute<T: Attribute>(app: &mut App) {
    if !vitality_core::first_install::<AttributeInstalled<T>>(app) {
        return;
    }
    init_core_attribute::<T>(app);
    init_effect_attribute::<T>(app);

    app.register_type::<Clamp<T>>();
    app.add_systems(
        FixedUpdate,
        apply_clamps::<T>
            .after(vitality_effects::systems::update_current_value_system::<T>)
            .in_set(EffectsSet::UpdateCurrentValues),
    );
    app.add_observer(clamp_dependency_changed::<T>);
}
