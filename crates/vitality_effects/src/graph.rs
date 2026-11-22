//! a modifier writes the actor named by its [`EffectSubject`](crate::modifier::EffectSubject)

use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

#[derive(Component, Default, Copy, Clone, Reflect, Debug)]
pub enum NodeType {
    #[default]
    Actor,
    Effect,
}
