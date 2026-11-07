mod application;
pub mod builder;
mod components;
pub mod condition;
pub mod conditions;
pub mod context;
pub mod definition;
mod global_effect;
pub mod graph;
pub mod modifier;
mod periodic;
pub mod registry;
mod stacks;
pub mod systems;
mod targeting;
mod timing;

use crate::application::apply_effect_event_observer;
use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::modifier::{
    ApplyAttributeModifierMessage, AttributeModifier, ModifierOf, apply_modifier_events,
    update_modifier_when_dependencies_changed,
};
use crate::periodic::apply_periodic_effect;
use crate::registry::EffectRegistry;
use crate::stacks::read_add_stack_event;
use crate::systems::{
    mark_node_dirty_observer, on_add_attribute, on_change_notify_attribute_dependencies,
    on_change_notify_attribute_parents, update_attribute, update_current_value_system,
};
use bevy::ecs::world::{EntityMutExcept, EntityRefExcept};
use bevy::prelude::*;
use vitality_core::attributes::Attribute;
use vitality_core::schedule::EffectsSet;

pub use application::*;
pub use builder::EffectBuilder;
pub use components::*;
pub use condition::ConditionPlugin;
pub use global_effect::*;
pub use stacks::*;
pub use targeting::EffectTargeting;
pub use timing::*;

pub mod prelude {
    pub use crate::context::EffectExprSchema;
    pub use crate::modifier::{AccessModifier, AttributeModifier, EffectSubject};
    pub use crate::{EffectApplicationPolicy, EffectBuilder};
    pub use vitality_core::prelude::*;
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(EffectRegistry::default())
            .init_asset::<EffectDef>()
            .register_type::<AppliedEffects>()
            .register_type::<EffectTarget>()
            .register_type::<NodeType>();

        app.add_systems(Update, tick_effect_tickers.in_set(EffectsSet::Prepare))
            .add_systems(Update, tick_effect_durations.in_set(EffectsSet::Prepare))
            .add_systems(Update, read_add_stack_event.in_set(EffectsSet::Prepare))
            .add_observer(apply_effect_event_observer)
            .add_message::<NotifyAddStackEvent>();
    }
}

/// Registers the effect-side systems for an attribute. Pair with `vitality_core::init_core_attribute`.
pub fn init_effect_attribute<T: Attribute>(app: &mut App) {
    app.register_type::<AttributeModifier<T>>();
    app.add_message::<ApplyAttributeModifierMessage<T>>();

    app.add_systems(
        FixedUpdate,
        (
            apply_periodic_effect::<T>.in_set(EffectsSet::Prepare),
            apply_modifier_events::<T>.in_set(EffectsSet::UpdateBaseValues),
            update_current_value_system::<T>.in_set(EffectsSet::UpdateCurrentValues),
            (
                on_change_notify_attribute_parents::<T>,
                on_change_notify_attribute_dependencies::<T>,
            )
                .in_set(EffectsSet::Notify),
        ),
    );

    app.add_observer(mark_node_dirty_observer::<T>);
    app.add_observer(on_add_attribute::<T>);
    app.add_observer(update_attribute::<T>);
    app.add_observer(update_modifier_when_dependencies_changed::<T>);
}

/// Attribute view for code that also touches effect components in the same system.
pub type EffectsMut<'w, 's> = EntityMutExcept<
    'w,
    's,
    (
        ChildOf,
        ModifierOf,
        (
            Effect,
            EffectDuration,
            EffectTicker,
            EffectSource,
            EffectTarget,
            AppliedEffects,
            EffectSources,
        ),
    ),
>;

pub type EffectsRef<'w, 's> = EntityRefExcept<
    'w,
    's,
    (
        ChildOf,
        ModifierOf,
        (
            Effect,
            EffectDuration,
            EffectTicker,
            EffectSource,
            EffectTarget,
            AppliedEffects,
            EffectSources,
        ),
    ),
>;
