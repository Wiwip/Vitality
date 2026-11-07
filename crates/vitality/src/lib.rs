use bevy::prelude::*;
use vitality_abilities::AbilityPlugin;
use vitality_core::CorePlugin;
use vitality_core::attributes::Attribute;
use vitality_core::effect::Stacks;
use vitality_core::init_core_attribute;
use vitality_effects::{ConditionPlugin, EffectsPlugin, GlobalEffectPlugin, init_effect_attribute};

pub use vitality_core::prelude;
pub use vitality_core::{
    AppAttributeBindings, AttributeBindings, AttributeError, AttributesMut, AttributesRef,
    BaseValueChanged, CurrentValueChanged, Dirty, attribute, attribute_impl, express_it,
    num_traits, tag,
};
pub use vitality_core::{
    actors, assets, attributes, context, graph, math, modifier, mutator, registry,
};

pub mod ability {
    pub use vitality_abilities::{Abilities, ActivateAbility};
    pub use vitality_core::ability::*;

    pub mod tasks {
        pub use vitality_abilities::tasks::*;
        pub use vitality_core::ability::tasks::*;
    }
}

pub mod condition {
    pub use vitality_core::condition::*;
    pub use vitality_effects::ConditionPlugin;
}

pub mod effect {
    pub use vitality_core::effect::*;
    pub use vitality_effects::EffectsPlugin;

    pub mod global_effect {
        pub use vitality_core::effect::global_effect::*;
        pub use vitality_effects::GlobalEffectPlugin;
    }
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
}
