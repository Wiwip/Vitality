use crate::actors::{ActorDef, ActorRegistry};
use crate::clamp::{Clamp, apply_clamps, update_clamps};
use bevy::prelude::*;
use vitality_abilities::AbilityPlugin;
use vitality_core::CorePlugin;
use vitality_core::attributes::Attribute;
use vitality_core::effect::Stacks;
use vitality_core::init_core_attribute;
use vitality_core::schedule::EffectsSet;
use vitality_effects::{ConditionPlugin, EffectsPlugin, GlobalEffectPlugin, init_effect_attribute};

pub use vitality_core::prelude;
pub use vitality_core::{
    AbilityMut, AbilityRef, AppAttributeBindings, AttributeBindings, AttributeError,
    BaseValueChanged, CurrentValueChanged, Dirty, EffectsMut, EffectsRef, attribute,
    attribute_impl, express_it, num_traits, tag,
};
pub use vitality_core::{assets, attributes, graph, math, modifier, mutator};

pub mod actors;
pub mod clamp;
mod vitality;

pub mod context {
    pub use crate::vitality::Vitality;
    pub use vitality_core::context::*;
}

pub mod registry {
    pub use crate::actors::{ActorRegistry, ActorToken};
    pub use vitality_core::registry::*;
}

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
            .after(vitality_core::systems::update_current_value_system::<T>)
            .in_set(EffectsSet::UpdateCurrentValues),
    );
    app.add_observer(update_clamps::<T>);
}
