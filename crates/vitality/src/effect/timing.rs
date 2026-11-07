use crate::effect::EffectInactive;
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
    pub(crate) fn new(timer: &Timer) -> EffectTicker {
        Self(timer.clone())
    }
}

/// Updates the duration timers for all active effects.
///
/// The function iterates over all entities that have an `EffectDuration` component,
/// excluding those with an `EffectInactive` component, and progresses their timers.
/// This is done in parallel for performance optimization.
pub fn tick_effect_durations(
    mut query: Query<(Entity, &mut EffectDuration), Without<EffectInactive>>,
    time: Res<Time>,
    par_commands: ParallelCommands,
) {
    query
        .par_iter_mut()
        .for_each(|(entity, mut effect_duration)| {
            effect_duration.0.tick(time.delta());

            // Remove expired effects
            if effect_duration.is_finished() {
                debug!("Effect expired on {}.", entity);
                par_commands.command_scope(|mut commands| {
                    commands.entity(entity).despawn();
                });
            }
        });
}

pub fn tick_effect_tickers(
    mut query: Query<&mut EffectTicker, Without<EffectInactive>>,
    time: Res<Time>,
) {
    query.par_iter_mut().for_each(|mut effect_ticker| {
        effect_ticker.0.tick(time.delta());
    });
}
