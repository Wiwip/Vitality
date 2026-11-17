//! a modifier applies to the attribute of the actor named by its [`EffectSubject`](crate::modifier::EffectSubject) — the target, or the source for a `Source` modifier

use bevy::prelude::*;

#[derive(Component, Default, Copy, Clone, Reflect, Debug)]
pub enum NodeType {
    #[default]
    Actor,
    Effect,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
