mod application;
mod builder;
pub mod global_effect;
mod stacks;
mod targeting;
mod timing;

use crate::assets::EffectDef;
use crate::prelude::Attribute;
use bevy::asset::Handle;
use bevy::ecs::query::QueryData;
use bevy::prelude::{Component, Deref, Entity, Event, Reflect};

pub use crate::effect::builder::EffectBuilder;
pub use application::{ApplyEffectEvent, EffectApplicationPolicy};
pub use global_effect::GlobalEffects;
pub use stacks::{EffectStackingPolicy, NotifyAddStackEvent, Stacks};
pub use targeting::EffectTargeting;
pub use timing::{EffectDuration, EffectTicker};

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
