use crate::actors::{ActorDef, ActorRegistry};
use crate::clamp::{Clamp, apply_clamps, update_clamps};
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
    attribute, attribute_impl, attributes, express_it, math, mutator, num_traits, tag,
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
    pub use vitality_abilities::prelude::*;
    pub use vitality_effects::prelude::*;
}

#[cfg(feature = "debug-overlay")]
pub mod inspector {
    pub use vitality_core::util::{pretty_type_name, pretty_type_name_str};
    pub use vitality_debug::*;
}

pub struct AttributesPlugin;

impl Plugin for AttributesPlugin {
    fn build(&self, app: &mut App) {
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

impl AttributesPlugin {
    pub fn default() -> Self {
        Self
    }
}

pub fn init_attribute<T: Attribute>(app: &mut App) {
    init_core_attribute::<T>(app);
    init_effect_attribute::<T>(app);

    app.register_type::<Clamp<T>>();
    app.add_systems(
        FixedUpdate,
        apply_clamps::<T>
            .after(vitality_effects::systems::update_current_value_system::<T>)
            .in_set(EffectsSet::UpdateCurrentValues),
    );
    app.add_observer(update_clamps::<T>);
}
