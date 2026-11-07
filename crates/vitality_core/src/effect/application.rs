use crate::assets::EffectDef;
use crate::effect::EffectTargeting;
use crate::effect::timing::{EffectDuration, EffectTicker};
use bevy::asset::Handle;
use bevy::prelude::*;
use std::cmp::PartialEq;

/// Describes how the effect is applied to entities
#[derive(Debug, Clone, Reflect, PartialEq)]
pub enum EffectApplicationPolicy {
    /// Applied once immediately
    Instant,

    /// Applied once and persists forever
    Permanent,

    /// Applied once, persists for a duration, then removed
    Temporary { duration: Timer },

    /// Applied repeatedly at intervals, forever
    Periodic { interval: Timer },

    /// Applied repeatedly at intervals for a limited time
    PeriodicTemporary { interval: Timer, duration: Timer },
}

impl EffectApplicationPolicy {
    // Constructor methods
    pub fn instant() -> Self {
        Self::Instant
    }

    pub fn permanent() -> Self {
        Self::Permanent
    }

    pub fn for_seconds(duration: f32) -> Self {
        Self::Temporary {
            duration: Timer::from_seconds(duration, TimerMode::Once),
        }
    }

    pub fn every_seconds(interval: f32) -> Self {
        Self::Periodic {
            interval: Timer::from_seconds(interval, TimerMode::Repeating),
        }
    }

    pub fn every_seconds_for_duration(interval: f32, duration: f32) -> Self {
        Self::PeriodicTemporary {
            interval: Timer::from_seconds(interval, TimerMode::Repeating),
            duration: Timer::from_seconds(duration, TimerMode::Once),
        }
    }

    // State checking methods
    pub fn is_expired(&self) -> bool {
        match self {
            Self::Instant => true,
            Self::Permanent | Self::Periodic { .. } => false,
            Self::Temporary { duration } => duration.is_finished(),
            Self::PeriodicTemporary { duration, .. } => duration.is_finished(),
        }
    }

    pub fn is_periodic(&self) -> bool {
        match self {
            Self::Instant | Self::Permanent | Self::Temporary { .. } => false,
            Self::Periodic { .. } | Self::PeriodicTemporary { .. } => true,
        }
    }

    pub fn should_apply_now(&self) -> bool {
        match self {
            Self::Instant => true,                             // Apply once on creation
            Self::Permanent | Self::Temporary { .. } => false, // Applied through aggregator systems
            Self::Periodic { interval } | Self::PeriodicTemporary { interval, .. } => {
                interval.just_finished()
            }
        }
    }

    pub fn reset(&mut self) {
        match self {
            Self::Temporary { duration } | Self::PeriodicTemporary { duration, .. } => {
                duration.reset();
            }
            _ => {}
        }
    }

    pub fn to_bundles(&self) -> (Option<impl Bundle>, Option<impl Bundle>) {
        let duration = match self {
            EffectApplicationPolicy::Temporary { duration } => Some(EffectDuration::new(duration)),
            EffectApplicationPolicy::PeriodicTemporary { duration, .. } => {
                Some(EffectDuration::new(duration))
            }
            _ => None,
        };

        let period = match self {
            EffectApplicationPolicy::Periodic { interval } => Some(EffectTicker::new(interval)),
            EffectApplicationPolicy::PeriodicTemporary { interval, .. } => {
                Some(EffectTicker::new(interval))
            }
            _ => None,
        };

        (duration, period)
    }
}

#[derive(EntityEvent)]
pub struct ApplyEffectEvent {
    pub entity: Entity,
    pub targeting: EffectTargeting,
    pub handle: Handle<EffectDef>,
}
