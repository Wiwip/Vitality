mod application;
pub mod builder;
mod components;
pub mod condition;
pub mod conditions;
pub mod context;
pub mod definition;
mod effects_param;
mod global_effect;
pub mod graph;
pub mod lifecycle;
pub mod modifier;
mod periodic;
pub mod registry;
pub mod resolver;
mod stacks;
pub mod systems;
mod targeting;
mod timing;

use crate::application::apply_effect_event_observer;
use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::modifier::{
    ApplyAttributeModifierMessage, AttributeModifier, ModifierApplies, ModifierOf,
    apply_all_modifier_events,
};
use crate::periodic::{PeriodicApplies, apply_all_periodic_effects};
use crate::registry::EffectRegistry;
use crate::stacks::read_add_stack_event;
use bevy::ecs::world::{EntityMutExcept, EntityRefExcept};
use bevy::prelude::*;
use vitality_core::attributes::Attribute;
use vitality_core::registry::scan_base_writes;
use vitality_core::schedule::EffectsSet;

pub use application::*;
pub use builder::EffectBuilder;
pub use components::*;
pub use condition::ConditionPlugin;
pub use effects_param::Effects;
pub use global_effect::*;
pub use lifecycle::{
    EffectApplied, EffectBlocked, EffectRemovalReason, EffectRemoved, EffectStacksChanged,
};
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
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        app.insert_resource(EffectRegistry::default())
            .init_asset::<EffectDef>()
            .register_type::<AppliedEffects>()
            .register_type::<EffectTarget>()
            .register_type::<NodeType>();

        // same clock as the resolver and the periodic appliers, so a timer that finishes is seen once
        app.add_systems(
            FixedUpdate,
            (
                tick_effect_tickers,
                tick_effect_durations,
                read_add_stack_event,
            )
                .chain()
                .in_set(EffectsSet::Prepare),
        )
        .add_observer(apply_effect_event_observer)
        .add_observer(lifecycle::on_effect_removed)
        .add_message::<NotifyAddStackEvent>();

        app.init_resource::<resolver::ModifierRecalcs>()
            .add_systems(
                FixedUpdate,
                resolver::resolve_system.in_set(EffectsSet::UpdateCurrentValues),
            )
            .add_observer(resolver::mark_actor_dirty_dag);
        // the base writes land here, so the collapsed scan and the walk see them the same tick
        app.add_systems(
            FixedUpdate,
            apply_all_modifier_events
                .in_set(EffectsSet::UpdateBaseValues)
                .before(scan_base_writes),
        );
        // one node for every attribute, and it has to see a base write before the walk does
        app.add_systems(
            FixedUpdate,
            scan_base_writes
                .in_set(EffectsSet::UpdateCurrentValues)
                .before(resolver::resolve_system),
        );
        // one node for every attribute again; the fire on the tick a duration expires still has
        // to be written, so the expiry that despawns the effect cannot win the tick
        app.add_systems(
            FixedUpdate,
            apply_all_periodic_effects
                .in_set(EffectsSet::Prepare)
                .after(tick_effect_tickers)
                .before(tick_effect_durations),
        );
    }
}

/// Registers the effect-side per-attribute state for one attribute: the collapsed nodes
/// `EffectsPlugin::build` installed cover every attribute, so this adds no system and no observer.
/// Pair with `vitality_core::init_core_attribute`.
pub fn init_effect_attribute<T: Attribute>(app: &mut App) {
    app.register_type::<AttributeModifier<T>>();
    app.add_message::<ApplyAttributeModifierMessage<T>>();

    app.world_mut()
        .get_resource_or_init::<resolver::ModifierRecalcs>()
        .register::<T>();
    app.world_mut()
        .get_resource_or_init::<ModifierApplies>()
        .register::<T>();
    app.world_mut()
        .get_resource_or_init::<PeriodicApplies>()
        .register::<T>();
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
