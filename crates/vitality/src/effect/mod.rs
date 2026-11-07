mod application;
mod builder;
pub mod global_effect;
mod stacks;
mod targeting;
mod timing;

use crate::assets::EffectDef;
use crate::effect::application::apply_effect_event_observer;
use crate::effect::stacks::{NotifyAddStackEvent, read_add_stack_event};
use crate::effect::timing::{tick_effect_durations, tick_effect_tickers};
use crate::prelude::Attribute;
use crate::schedule::EffectsSet;
use bevy::app::{App, Plugin};
use bevy::asset::Handle;
use bevy::ecs::query::QueryData;
use bevy::prelude::{Component, Deref, Entity, Event, IntoScheduleConfigs, Reflect, Update};
use std::marker::PhantomData;

pub use crate::effect::builder::EffectBuilder;
pub use application::{ApplyEffectEvent, EffectApplicationPolicy};
pub use global_effect::GlobalEffects;
pub use stacks::{EffectStackingPolicy, Stacks};
pub use targeting::EffectTargeting;
pub use timing::{EffectDuration, EffectTicker};

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

pub enum EffectStatus {
    Active,
    Inactive,
}

#[derive(Clone, Copy, Debug, Reflect)]
pub enum Target {
    SelfEntity,
    TargetEntity,
}

#[derive(Event)]
pub struct OnEffectStatusChangeEvent(pub EffectStatus);

#[derive(Component, Debug, Default)]
#[component(storage = "SparseSet")]
pub struct EffectInactive;

#[derive(Component, Debug, Default, Deref)]
#[require(Stacks)]
pub struct Effect(pub Handle<EffectDef>);

impl Effect {
    pub fn instant() -> EffectBuilder {
        EffectBuilder::new(EffectApplicationPolicy::Instant)
    }

    pub fn permanent() -> EffectBuilder {
        EffectBuilder::new(EffectApplicationPolicy::Permanent)
    }

    pub fn temporary(duration: f32) -> EffectBuilder {
        EffectBuilder::new(EffectApplicationPolicy::for_seconds(duration))
    }

    pub fn permanent_ticking(tick_rate_secs: f32) -> EffectBuilder {
        EffectBuilder::new(EffectApplicationPolicy::every_seconds(tick_rate_secs))
    }

    pub fn temporary_ticking(tick_rate_secs: f32, duration: f32) -> EffectBuilder {
        EffectBuilder::new(EffectApplicationPolicy::every_seconds_for_duration(
            tick_rate_secs,
            duration,
        ))
    }
}

/// What are the attributes the modifier depends on?
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = AttributeDependents<T>)]
pub struct AttributeDependency<T: Attribute + 'static> {
    #[relationship]
    pub source: Entity,
    marker: PhantomData<T>,
}

impl<T: Attribute> AttributeDependency<T> {
    pub fn new(source: Entity) -> Self {
        Self {
            source,
            marker: PhantomData,
        }
    }
}

/// Usually on actors. Who depends on this entity and for what attributes?
/// Serves to notify derived modifiers that the attribute has changed.
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = AttributeDependency<T>)]
pub struct AttributeDependents<T: Attribute + 'static> {
    #[relationship]
    sources: Vec<Entity>,
    marker: PhantomData<T>,
}

/// The source of this effect.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = EffectSources)]
pub struct EffectSource(pub Entity);

/// All effects originating from this entity.
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = EffectSource, linked_spawn)]
pub struct EffectSources(Vec<Entity>);

/// The target entity of this effect.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = AppliedEffects)]
pub struct EffectTarget(pub Entity);

/// All effects that are targeting this entity.
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = EffectTarget, linked_spawn)]
pub struct AppliedEffects(Vec<Entity>);

#[derive(QueryData)]
pub struct EffectStatusParam {
    inactive: Option<&'static EffectInactive>,
    periodic: Option<&'static EffectTicker>,
}

impl EffectStatusParamItem<'_, '_> {
    pub fn is_inactive(&self) -> bool {
        self.inactive.is_some()
    }
    pub fn is_periodic(&self) -> bool {
        self.periodic.is_some()
    }
}
