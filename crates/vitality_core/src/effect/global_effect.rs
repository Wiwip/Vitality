use crate::assets::EffectDef;
use bevy::prelude::*;

#[derive(Resource, Deref, DerefMut, Default)]
pub struct GlobalEffects(Vec<Handle<EffectDef>>);

#[derive(Component, Clone, Copy)]
pub struct GlobalActor;

#[derive(Component, Clone, Copy)]
pub struct GlobalEffect;
