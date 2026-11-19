//! an instant or periodic modifier writes the attribute of the actor named by its [`EffectSubject`](crate::modifier::EffectSubject) — the target, or the source for a `Source` modifier; a persistent effect folds every modifier into the actor that carries it, so a `Source` modifier there writes the target and warns when it is applied

use bevy::prelude::*;

#[derive(Component, Default, Copy, Clone, Reflect, Debug)]
pub enum NodeType {
    #[default]
    Actor,
    Effect,
}
