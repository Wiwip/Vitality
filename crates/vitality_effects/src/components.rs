use crate::definition::EffectDef;
use crate::{EffectApplicationPolicy, EffectBuilder, Stacks};
use bevy_derive::Deref;
use bevy_ecs::prelude::{Component, Entity};
use bevy_reflect::Reflect;
use vitality_core::store::Handle;

#[derive(Component, Debug, Default)]
#[component(storage = "SparseSet")]
pub struct EffectInactive;

/// the def can change this effect's state on its own, so the condition walk keeps reading it
#[derive(Component, Debug, Default)]
#[component(storage = "SparseSet")]
pub struct EffectConditional;

#[derive(Component, Debug, Default, Deref)]
#[require(Stacks)]
pub struct Effect(pub Handle<EffectDef>);

/// Type ids of the tags an effect carries, for erased lookups like cleansing.
#[derive(Component, Debug, Default, Clone)]
pub struct EffectTags(pub Vec<std::any::TypeId>);

impl AppliedEffects {
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

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

#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = EffectSources)]
pub struct EffectSource(pub Entity);

#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = EffectSource, linked_spawn)]
pub struct EffectSources(Vec<Entity>);

#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = AppliedEffects)]
pub struct EffectTarget(pub Entity);

#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = EffectTarget, linked_spawn)]
pub struct AppliedEffects(Vec<Entity>);
