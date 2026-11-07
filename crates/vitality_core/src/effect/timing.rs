use bevy::prelude::*;
use bevy::time::Timer;

#[derive(Component, Deref, DerefMut)]
pub struct EffectDuration(pub Timer);

impl EffectDuration {
    pub fn new(timer: &Timer) -> EffectDuration {
        Self(timer.clone())
    }
}

#[derive(Component, Deref, DerefMut)]
pub struct EffectTicker(pub Timer);

impl EffectTicker {
    pub fn new(timer: &Timer) -> EffectTicker {
        Self(timer.clone())
    }
}
