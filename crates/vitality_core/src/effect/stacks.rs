use crate::assets::EffectDef;
use crate::attribute;
use crate::prelude::Attribute;
use crate::{ReflectAccessAttribute, attribute_impl};
use bevy::prelude::*;

pub enum EffectStackingPolicy {
    None, // Each effect is independently added to the entity
    Add { count: u32, max_stack: u32 },
    RefreshDuration, // The effect overrides previous applications
                     //RefreshDurationWithOverflow, // The effect overrides previous applications
}
attribute!(EffectIntensity, f32);

attribute_impl!(Stacks, u32);

impl Default for Stacks {
    fn default() -> Self {
        Stacks::new(1) // By default, a new effect has 1 stack
    }
}

#[derive(Message)]
pub struct NotifyAddStackEvent {
    pub effect_entity: Entity,
    pub handle: Handle<EffectDef>,
}
