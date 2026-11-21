use crate::EffectInactive;
use crate::lifecycle::{EffectRemovalReason, remove_effect};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_time::Timer;
use bevy_time::prelude::*;
use bevy_utils::Parallel;
use tracing::debug;

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

pub fn tick_effect_durations(
    mut query: Query<(Entity, &mut EffectDuration), Without<EffectInactive>>,
    time: Res<Time>,
    mut expired: Local<Parallel<Vec<Entity>>>,
    mut gone: Local<Vec<Entity>>,
    mut commands: Commands,
) {
    query
        .par_iter_mut()
        .for_each(|(entity, mut effect_duration)| {
            effect_duration.0.tick(time.delta());

            if effect_duration.is_finished() {
                expired.borrow_local_mut().push(entity);
            }
        });

    // removed on this thread in entity order, so the removals do not follow the thread count
    gone.clear();
    expired.drain_into(&mut gone);
    gone.sort();
    for &entity in gone.iter() {
        debug!("Effect expired on {}.", entity);
        remove_effect(&mut commands, entity, EffectRemovalReason::Expired);
    }
}

pub fn tick_effect_tickers(
    mut query: Query<&mut EffectTicker, Without<EffectInactive>>,
    time: Res<Time>,
) {
    query.par_iter_mut().for_each(|mut effect_ticker| {
        effect_ticker.0.tick(time.delta());
    });
}
