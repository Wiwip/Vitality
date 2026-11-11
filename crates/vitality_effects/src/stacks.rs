use crate::definition::EffectDef;
use crate::lifecycle::EffectStacksChanged;
use crate::systems::MarkActorDirty;
use crate::{Effect, EffectDuration, EffectTarget};
use bevy::prelude::*;
use vitality_core::attribute;
use vitality_core::attribute_impl;
use vitality_core::attributes::Attribute;
use vitality_core::attributes::ReflectAccessAttribute;

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

impl EffectStackingPolicy {
    /// Returns the (old, new) stack count when it changed.
    fn apply(
        &self,
        effect_entity: Entity,
        stacks: &mut Query<&mut Stacks, With<Effect>>,
        durations: &mut Query<&mut EffectDuration, With<Effect>>,
    ) -> Option<(u32, u32)> {
        match self {
            EffectStackingPolicy::Add { count, max_stack } => {
                let Ok(mut stack_count) = stacks.get_mut(effect_entity) else {
                    warn!("Effect {effect_entity} has no Stacks.");
                    return None;
                };
                let old = stack_count.current_value();
                let new = (old + count).clamp(1, (*max_stack).max(1));
                // a capped add still refreshes the duration, like a reapplication would
                if let Ok(mut duration) = durations.get_mut(effect_entity) {
                    duration.reset();
                }
                if new == old {
                    return None;
                }
                stack_count.set_base_value(new);
                stack_count.set_current_value(new);
                Some((old, new))
            }
            EffectStackingPolicy::RefreshDuration => {
                if let Ok(mut duration) = durations.get_mut(effect_entity) {
                    duration.reset();
                }
                None
            }
            EffectStackingPolicy::None => None,
        }
    }
}

/// Sets an effect's stack count directly; zero removes the effect.
pub(crate) fn set_stacks(
    effect_entity: Entity,
    count: u32,
    stacks: &mut Query<&mut Stacks, With<Effect>>,
) -> Option<(u32, u32)> {
    let mut stack_count = stacks.get_mut(effect_entity).ok()?;
    let old = stack_count.current_value();
    if old == count {
        return None;
    }
    stack_count.set_base_value(count);
    stack_count.set_current_value(count);
    Some((old, count))
}

pub(crate) fn read_add_stack_event(
    mut event_reader: MessageReader<NotifyAddStackEvent>,
    mut stacks: Query<&mut Stacks, With<Effect>>,
    mut applications: Query<&mut EffectDuration, With<Effect>>,
    targets: Query<&EffectTarget>,
    effect_assets: Res<Assets<EffectDef>>,
    mut commands: Commands,
) {
    for ev in event_reader.read() {
        let Some(effect_definition) = effect_assets.get(&ev.handle) else {
            warn!("No effect definition for {:?}", ev.handle);
            continue;
        };
        let Some((old, new)) = effect_definition.stacking_policy.apply(
            ev.effect_entity,
            &mut stacks,
            &mut applications,
        ) else {
            continue;
        };
        notify_stacks_changed(ev.effect_entity, old, new, &targets, &mut commands);
    }
}

pub(crate) fn notify_stacks_changed(
    effect: Entity,
    old: u32,
    new: u32,
    targets: &Query<&EffectTarget>,
    commands: &mut Commands,
) {
    let Ok(target) = targets.get(effect) else {
        return;
    };
    commands.trigger(MarkActorDirty { entity: target.0 });
    commands.trigger(EffectStacksChanged {
        entity: target.0,
        effect,
        old,
        new,
    });
}
