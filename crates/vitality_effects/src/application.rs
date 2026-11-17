use crate::EffectTags;
use crate::EffectTargeting;
use crate::EffectsRef;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::lifecycle::{EffectApplied, EffectBlocked, EffectRemovalReason, remove_effect};
use crate::modifier::ModifierOf;
use crate::systems::MarkActorDirty;
use crate::timing::{EffectDuration, EffectTicker};
use crate::{
    AppliedEffects, Effect, EffectSource, EffectStackingPolicy, EffectTarget, NotifyAddStackEvent,
};
use bevy::asset::Assets;
use bevy::asset::Handle;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::cmp::PartialEq;
use vitality_core::AppAttributeBindings;
use vitality_core::tags::ChangeTag;
use vitality_core::view::ActorRead;

#[derive(Debug, Clone, Reflect, PartialEq)]
pub enum EffectApplicationPolicy {
    Instant,

    Permanent,

    Temporary { duration: Timer },

    Periodic { interval: Timer },

    PeriodicTemporary { interval: Timer, duration: Timer },
}

impl EffectApplicationPolicy {
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

type ActorQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static AppliedEffects>,
        EffectsRef<'static, 'static>,
    ),
    (Without<Effect>, Without<IsResource>),
>;

impl ApplyEffectEvent {
    fn context<'a>(
        &self,
        actors: &'a ActorQuery,
    ) -> Option<(Option<&'a AppliedEffects>, EffectExprContext<'a, 'a>)> {
        let Ok((_, source)) = actors.get(self.targeting.source()) else {
            warn!("Failed to get source actor: {}", self.targeting.source());
            return None;
        };
        let Ok((applied, target)) = actors.get(self.targeting.target()) else {
            warn!("Failed to get target actor: {}", self.targeting.target());
            return None;
        };
        Some((
            applied,
            EffectExprContext {
                target_actor: Some(target),
                source_actor: source,
                effect_holder: source,
            },
        ))
    }

    fn passes_tag_rules(&self, effect: &EffectDef, ctx: &EffectExprContext) -> bool {
        let Some(target) = &ctx.target_actor else {
            return true;
        };
        effect
            .required_tags
            .iter()
            .all(|tag| target.has_type(tag.type_id))
            && !effect
                .blocked_by_tags
                .iter()
                .any(|tag| target.has_type(tag.type_id))
    }

    fn cleanse(
        &self,
        effect: &EffectDef,
        applied: Option<&AppliedEffects>,
        effect_tags: &Query<&EffectTags>,
        commands: &mut Commands,
    ) {
        if effect.cleanses.is_empty() {
            return;
        }
        let Some(applied) = applied else {
            return;
        };
        for other in applied.iter() {
            let Ok(tags) = effect_tags.get(other) else {
                continue;
            };
            if effect
                .cleanses
                .iter()
                .any(|tag| tags.0.contains(&tag.type_id))
            {
                remove_effect(commands, other, EffectRemovalReason::Cleansed);
            }
        }
    }

    /// a stacking policy lands on the copy already applied, so the spawn has nothing left to do; true when that is what happened
    fn queue_stacked_effect(
        &self,
        effect: &EffectDef,
        applied: Option<&AppliedEffects>,
        effects: &Query<&Effect>,
        add_stack_event: &mut MessageWriter<NotifyAddStackEvent>,
    ) -> bool {
        if matches!(effect.stacking_policy, EffectStackingPolicy::None) {
            return false;
        }
        let existing = applied.and_then(|applied| {
            applied.iter().find(|other| {
                effects
                    .get(*other)
                    .is_ok_and(|other| other.0.id() == self.handle.id())
            })
        });
        let Some(existing) = existing else {
            return false;
        };
        add_stack_event.write(NotifyAddStackEvent {
            effect_entity: existing,
            handle: self.handle.clone(),
        });
        true
    }

    fn spawn_persistent_effect(
        &self,
        commands: &mut Commands,
        effect: &EffectDef,
        context: &EffectExprContext,
        type_bindings: &AppAttributeBindings,
    ) -> Option<Entity> {
        if !effect
            .attach_conditions
            .iter()
            .all(|condition| condition.eval(context))
        {
            return None;
        }

        let target = self.targeting.target();
        let mut effect_commands = commands.spawn_empty();
        let effect_entity = effect_commands.id();
        for effect_fn in &effect.effect_fn {
            effect_fn(&mut effect_commands, target);
        }

        effect_commands.insert((
            NodeType::Effect,
            EffectTarget(target),
            EffectSource(self.targeting.source()),
            Effect(self.handle.clone()),
            EffectTags(effect.granted_tags.iter().map(|tag| tag.type_id).collect()),
        ));
        for &tag in &effect.granted_tags {
            effect_commands.queue(move |mut entity: EntityWorldMut| tag.insert_on(&mut entity));
        }

        let (duration, ticker) = effect.application_policy.to_bundles();
        if let Some(duration) = duration {
            effect_commands.insert(duration);
        }
        if let Some(ticker) = ticker {
            effect_commands.insert(ticker);
        }
        for triggers in &effect.on_effect_triggers {
            triggers.apply(&mut effect_commands);
        }

        let bindings = type_bindings
            .internal
            .read()
            .unwrap_or_else(|err| err.into_inner());
        for modifier in &effect.modifiers {
            let mut entity_commands = commands.spawn(ModifierOf(effect_entity));
            modifier.spawn_persistent_modifier(
                &self.targeting,
                context,
                &bindings,
                &mut entity_commands,
            );
        }

        let mut target_commands = commands.entity(target);
        for tag in &effect.granted_tags {
            target_commands.queue_silenced(ChangeTag::grant(*tag));
        }
        for triggers in &effect.on_actor_triggers {
            triggers.apply(&mut target_commands);
        }

        Some(effect_entity)
    }
}

#[derive(SystemParam)]
pub(crate) struct ApplyContext<'w, 's> {
    effects: Query<'w, 's, &'static Effect>,
    effect_tags: Query<'w, 's, &'static EffectTags>,
    effect_assets: Res<'w, Assets<EffectDef>>,
    type_bindings: Res<'w, AppAttributeBindings>,
}

pub(crate) fn apply_effect_event_observer(
    trigger: On<ApplyEffectEvent>,
    actors: ActorQuery,
    ctx: ApplyContext<'_, '_>,
    mut writer: MessageWriter<NotifyAddStackEvent>,
    mut commands: Commands,
) {
    let Some(effect) = ctx.effect_assets.get(&trigger.handle) else {
        warn!("Effect {:?} has no loaded definition.", trigger.handle);
        return;
    };
    let Some((applied, context)) = trigger.context(&actors) else {
        return;
    };
    let source = trigger.targeting.source();
    let target = trigger.targeting.target();

    if !trigger.passes_tag_rules(effect, &context) {
        commands.trigger(EffectBlocked {
            entity: target,
            source,
            handle: trigger.handle.clone(),
        });
        return;
    }

    trigger.cleanse(effect, applied, &ctx.effect_tags, &mut commands);

    if effect.application_policy == EffectApplicationPolicy::Instant {
        if !effect
            .activate_conditions
            .iter()
            .all(|condition| condition.eval(&context))
        {
            return;
        }
        for modifier in &effect.modifiers {
            modifier.apply_delayed(source, target, trigger.entity, &mut commands);
        }
        commands.trigger(EffectApplied {
            entity: target,
            source,
            effect: None,
            handle: trigger.handle.clone(),
        });
        return;
    }

    if trigger.queue_stacked_effect(effect, applied, &ctx.effects, &mut writer) {
        return;
    }

    if let Some(effect_entity) =
        trigger.spawn_persistent_effect(&mut commands, effect, &context, &ctx.type_bindings)
    {
        commands.trigger(MarkActorDirty { entity: target });
        commands.trigger(EffectApplied {
            entity: target,
            source,
            effect: Some(effect_entity),
            handle: trigger.handle.clone(),
        });
    }
}
