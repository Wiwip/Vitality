mod application;
mod condition;
mod global_effect;
mod periodic;
mod stacks;
mod timing;

use crate::application::apply_effect_event_observer;
use crate::periodic::apply_periodic_effect;
use crate::stacks::read_add_stack_event;
use crate::timing::{tick_effect_durations, tick_effect_tickers};
use bevy::prelude::*;
use vitality_core::attributes::Attribute;
use vitality_core::effect::NotifyAddStackEvent;
use vitality_core::schedule::EffectsSet;

pub use condition::ConditionPlugin;
pub use global_effect::GlobalEffectPlugin;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, tick_effect_tickers.in_set(EffectsSet::Prepare))
            .add_systems(Update, tick_effect_durations.in_set(EffectsSet::Prepare))
            .add_systems(Update, read_add_stack_event.in_set(EffectsSet::Prepare))
            .add_observer(apply_effect_event_observer)
            .add_message::<NotifyAddStackEvent>();
    }
}

/// Registers the effect-side systems for an attribute. Pair with `vitality_core::init_core_attribute`.
pub fn init_effect_attribute<T: Attribute>(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        apply_periodic_effect::<T>.in_set(EffectsSet::Prepare),
    );
}
