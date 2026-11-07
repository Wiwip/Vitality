use crate::EffectTargeting;
use crate::EffectsMut;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::modifier::Modifier;
use crate::modifier::ModifierOf;
use crate::timing::{EffectDuration, EffectTicker};
use crate::{
    AppliedEffects, Effect, EffectSource, EffectStackingPolicy, EffectTarget, NotifyAddStackEvent,
};
use bevy::asset::Assets;
use bevy::asset::Handle;
use bevy::ecs::resource::IsResource;
use bevy::log::debug;
use bevy::prelude::*;
use std::cmp::PartialEq;
use vitality_core::AppAttributeBindings;

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

fn apply_instant_effect(
    event: &ApplyEffectEvent,
    actors: &mut Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    commands: &mut Commands,
    effect: &EffectDef,
) -> Result<(), BevyError> {
    debug!("Applying instant effect to {}", event.targeting.target());

    let Ok((_, source_actor_ref)) = actors.get(event.targeting.source()) else {
        warn!("Failed to get source actor: {}", event.targeting.source());
        return Ok(());
    };
    let Ok((_, target_actor_ref)) = actors.get(event.targeting.target()) else {
        warn!("Failed to get target actor: {}", event.targeting.target());
        return Ok(());
    };

    let context = EffectExprContext {
        target_actor: Some(target_actor_ref),
        source_actor: source_actor_ref,
        effect_holder: source_actor_ref, // TODO: Make optional
    };

    // Determines whether the effect should activate
    let should_apply = effect
        .activate_conditions
        .iter()
        .all(|condition| condition.eval(&context));

    if !should_apply {
        debug!("Effect not applied.");
        return Ok(());
    }

    apply_modifiers(event, actors, &mut effect.modifiers.iter(), commands);
    Ok(())
}

fn apply_modifiers<'a, I>(
    event: &ApplyEffectEvent,
    _actors: &'a mut Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    modifiers: &mut I,
    commands: &mut Commands,
) where
    I: Iterator<Item = &'a Box<dyn Modifier>>,
{
    for modifier in modifiers {
        modifier.apply_delayed(
            event.targeting.source(),
            event.targeting.target(),
            event.entity,
            commands,
        );
    }
}

fn spawn_persistent_effect(
    event: &ApplyEffectEvent,
    commands: &mut Commands,
    effect: &EffectDef,
    actors: &mut Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    effects: &mut Query<&Effect>,
    add_stack_event: &mut MessageWriter<NotifyAddStackEvent>,
    type_bindings: AppAttributeBindings,
) -> Result<(), BevyError> {
    // We want to know whether an effect with the same handle already points to the actor
    let (optional_effects, _) = actors.get_mut(event.targeting.target())?;
    let effects_on_actor = match optional_effects {
        None => {
            vec![]
        }
        Some(effects_on_actor) => {
            let effects = effects_on_actor.iter().filter_map(|effect_entity| {
                let Ok(other_effect) = effects.get(effect_entity) else {
                    return None;
                };
                if other_effect.0.id() == event.handle.id() {
                    Some(effect_entity)
                } else {
                    None
                }
            });
            effects.collect::<Vec<_>>()
        }
    };

    match effect.stacking_policy {
        EffectStackingPolicy::None => {
            // Continue spawning effect
        }
        EffectStackingPolicy::Add { .. } | EffectStackingPolicy::RefreshDuration => {
            if effects_on_actor.len() > 0 {
                debug!("Effect already exists on actor. Adding stacks per definition.");
                add_stack_event.write(NotifyAddStackEvent {
                    effect_entity: *effects_on_actor.first().unwrap(),
                    handle: event.handle.clone(),
                });
                return Ok(());
            }
        }
    }

    let (_, source_actor_ref) = actors.get(event.targeting.source())?;
    let (_, target_actor_ref) = actors.get(event.targeting.target())?;

    let context = EffectExprContext {
        target_actor: Some(target_actor_ref),
        source_actor: source_actor_ref,
        effect_holder: source_actor_ref, // TODO: Should this be the source actor? The effect doesn't exist for instant effects.
    };

    // Determines whether the effect should activate
    let should_be_applied = effect
        .attach_conditions
        .iter()
        .all(|condition| condition.eval(&context));

    if !should_be_applied {
        return Ok(());
    }

    let mut effect_commands = commands.spawn_empty();
    let effect_entity = effect_commands.id();
    for effect_fn in &effect.effect_fn {
        effect_fn(&mut effect_commands, event.targeting.target());
    }

    // Spawns the effect entity
    effect_commands.insert((
        NodeType::Effect,
        EffectTarget(event.targeting.target()),
        EffectSource(event.targeting.source()),
        Effect(event.handle.clone()),
    ));

    // Converts the policy to components that can be added to the entity
    let (duration, ticker) = effect.application_policy.to_bundles();
    if let Some(duration) = duration {
        effect_commands.insert(duration);
    }
    if let Some(ticker) = ticker {
        effect_commands.insert(ticker);
    }

    // Spawn effect modifiers
    let bindings = type_bindings.internal.read().unwrap();
    effect.modifiers.iter().for_each(|modifier| {
        let mut entity_commands = commands.spawn(ModifierOf(effect_entity));
        modifier.spawn_persistent_modifier(
            source_actor_ref.id(),
            &context,
            &bindings,
            &mut entity_commands,
        );
    });

    // Spawn effect triggers
    for triggers in &effect.on_actor_triggers {
        let mut entity_commands = commands.entity(event.entity);
        triggers.apply(&mut entity_commands);
    }

    // Spawn effect triggers
    for triggers in &effect.on_effect_triggers {
        let mut entity_commands = commands.entity(event.targeting.target());
        triggers.apply(&mut entity_commands);
    }

    Ok(())
}

pub(crate) fn apply_effect_event_observer(
    trigger: On<ApplyEffectEvent>,
    mut actors: Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    mut effects: Query<&Effect>,
    effect_assets: Res<Assets<EffectDef>>,
    mut writer: MessageWriter<NotifyAddStackEvent>,
    mut commands: Commands,
    type_bindings: Res<AppAttributeBindings>,
) -> Result<(), BevyError> {
    let effect = effect_assets
        .get(&trigger.handle)
        .ok_or("No effect asset.")?;

    if effect.application_policy.should_apply_now() {
        apply_instant_effect(&trigger, &mut actors, &mut commands, effect)?;
    }

    if effect.application_policy != EffectApplicationPolicy::Instant {
        spawn_persistent_effect(
            &trigger,
            &mut commands,
            effect,
            &mut actors,
            &mut effects,
            &mut writer,
            type_bindings.clone(),
        )?;
    }

    Ok(())
}
