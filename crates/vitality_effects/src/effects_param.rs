use crate::definition::EffectDef;
use crate::lifecycle::{EffectRemovalReason, remove_effect};
use crate::stacks::{notify_stacks_changed, set_stacks};
use crate::{
    AppliedEffects, ApplyEffectEvent, Effect, EffectDuration, EffectSource, EffectTags,
    EffectTarget, EffectTargeting, Stacks,
};
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemParam, SystemState};
use std::any::TypeId;
use std::time::Duration;
use vitality_core::attributes::Attribute;
use vitality_core::store::Handle;

type EffectInfo = (
    &'static Effect,
    &'static EffectSource,
    Option<&'static EffectTags>,
    Option<&'static Stacks>,
    Option<&'static EffectDuration>,
);

/// Applies, inspects and removes effects. Every change is deferred to the next command flush.
#[derive(SystemParam)]
pub struct Effects<'w, 's> {
    applied: Query<'w, 's, &'static AppliedEffects>,
    effects: Query<'w, 's, EffectInfo>,
    commands: Commands<'w, 's>,
}

impl Effects<'_, '_> {
    pub fn apply(&mut self, source: Entity, target: Entity, handle: &Handle<EffectDef>) {
        self.commands.trigger(ApplyEffectEvent {
            entity: target,
            targeting: EffectTargeting::new(source, target),
            handle: *handle,
        });
    }

    pub fn apply_to_self(&mut self, actor: Entity, handle: &Handle<EffectDef>) {
        self.apply(actor, actor, handle);
    }

    /// Effect entities currently applied to `target`.
    pub fn on(&self, target: Entity) -> Vec<Entity> {
        self.applied
            .get(target)
            .map(|applied| applied.iter().collect())
            .unwrap_or_default()
    }

    pub fn find(&self, target: Entity, handle: &Handle<EffectDef>) -> Option<Entity> {
        self.matching(target, |effect, _, _| effect.0.id() == handle.id())
            .into_iter()
            .next()
    }

    pub fn has(&self, target: Entity, handle: &Handle<EffectDef>) -> bool {
        self.find(target, handle).is_some()
    }

    pub fn stacks(&self, effect: Entity) -> Option<u32> {
        let (_, _, _, stacks, _) = self.effects.get(effect).ok()?;
        Some(stacks.map_or(1, |stacks| stacks.current_value()))
    }

    /// Time left before a temporary effect expires; None for effects without a duration.
    pub fn remaining(&self, effect: Entity) -> Option<Duration> {
        let (_, _, _, _, duration) = self.effects.get(effect).ok()?;
        duration.map(|duration| duration.remaining())
    }

    pub fn remove(&mut self, effect: Entity) {
        remove_effect(&mut self.commands, effect, EffectRemovalReason::Removed);
    }

    pub fn remove_by_def(&mut self, target: Entity, handle: &Handle<EffectDef>) -> usize {
        let id = handle.id();
        self.remove_matching(target, |effect, _, _| effect.0.id() == id)
    }

    pub fn remove_with_tag<T: Component>(&mut self, target: Entity) -> usize {
        let tag = TypeId::of::<T>();
        self.remove_matching(target, |_, _, tags| {
            tags.is_some_and(|tags| tags.0.contains(&tag))
        })
    }

    pub fn remove_from_source(&mut self, target: Entity, source: Entity) -> usize {
        self.remove_matching(target, |_, effect_source, _| effect_source.0 == source)
    }

    /// Takes `count` stacks off an effect, removing it when none are left.
    pub fn remove_stacks(&mut self, effect: Entity, count: u32) {
        let Some(current) = self.stacks(effect) else {
            return;
        };
        if count >= current {
            self.remove(effect);
            return;
        }
        let left = current - count;
        self.commands.queue(move |world: &mut World| {
            let mut state = SystemState::<(
                Query<&mut Stacks, With<Effect>>,
                Query<&EffectTarget>,
                Commands,
            )>::new(world);
            let Ok((mut stacks, targets, mut commands)) = state.get_mut(world) else {
                return;
            };
            if let Some((old, new)) = set_stacks(effect, left, &mut stacks) {
                notify_stacks_changed(effect, old, new, &targets, &mut commands);
            }
            state.apply(world);
        });
    }

    fn matching(
        &self,
        target: Entity,
        pick: impl Fn(&Effect, &EffectSource, Option<&EffectTags>) -> bool,
    ) -> Vec<Entity> {
        self.on(target)
            .into_iter()
            .filter(|&entity| {
                self.effects
                    .get(entity)
                    .is_ok_and(|(effect, source, tags, _, _)| pick(effect, source, tags))
            })
            .collect()
    }

    fn remove_matching(
        &mut self,
        target: Entity,
        pick: impl Fn(&Effect, &EffectSource, Option<&EffectTags>) -> bool,
    ) -> usize {
        let found = self.matching(target, pick);
        for &effect in &found {
            self.remove(effect);
        }
        found.len()
    }
}
