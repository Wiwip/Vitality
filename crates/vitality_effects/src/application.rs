use crate::EffectTags;
use crate::EffectTargeting;
use crate::EffectsRef;
use crate::conditions::ConditionRng;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::lifecycle::{EffectApplied, EffectBlocked, EffectRemovalReason, remove_effect};
use crate::modifier::ModifierOf;
use crate::systems::{DirtyMarks, MarkActorDirty};
use crate::timing::{EffectDuration, EffectTicker};
use crate::{
    AppliedEffects, Effect, EffectConditional, EffectSource, EffectStackingPolicy, EffectTarget,
    NotifyAddStackEvent,
};
use bevy_ecs::prelude::*;
use bevy_ecs::resource::IsResource;
use bevy_ecs::system::SystemParam;
use bevy_reflect::prelude::*;
use bevy_time::prelude::*;
use std::cmp::PartialEq;
use tracing::warn;
use vitality_core::AppAttributeBindings;
use vitality_core::attributes::ManageAttributes;
use vitality_core::registry::AttributeRegistry;
use vitality_core::store::Assets;
use vitality_core::store::Handle;
use vitality_core::tags::ChangeTag;
use vitality_core::view::ActorRead;
use vitality_core::warn_once;

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
        let method = "EffectApplicationPolicy::for_seconds";
        Self::Temporary {
            duration: Timer::from_seconds(checked_seconds(method, duration), TimerMode::Once),
        }
    }

    pub fn every_seconds(interval: f32) -> Self {
        let method = "EffectApplicationPolicy::every_seconds";
        Self::Periodic {
            interval: Timer::from_seconds(checked_seconds(method, interval), TimerMode::Repeating),
        }
    }

    pub fn every_seconds_for_duration(interval: f32, duration: f32) -> Self {
        let method = "EffectApplicationPolicy::every_seconds_for_duration";
        Self::PeriodicTemporary {
            interval: Timer::from_seconds(checked_seconds(method, interval), TimerMode::Repeating),
            duration: Timer::from_seconds(checked_seconds(method, duration), TimerMode::Once),
        }
    }

    pub fn is_periodic(&self) -> bool {
        match self {
            Self::Instant | Self::Permanent | Self::Temporary { .. } => false,
            Self::Periodic { .. } | Self::PeriodicTemporary { .. } => true,
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

    fn to_bundles(&self) -> (Option<impl Bundle>, Option<impl Bundle>) {
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

fn checked_seconds(method: &str, seconds: f32) -> f32 {
    if !seconds.is_finite() || seconds < 0.0 {
        panic!("`{method}` needs a finite number of seconds of 0 or more, got {seconds}");
    }
    seconds
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
        Has<ManageAttributes>,
    ),
    (Without<Effect>, Without<IsResource>),
>;

fn has_any_attribute(target: &EffectsRef<'_, '_>, registry: &AttributeRegistry) -> bool {
    registry
        .infos()
        .iter()
        .any(|info| target.has_type(info.type_id))
}

impl ApplyEffectEvent {
    fn context<'a>(
        &self,
        actors: &'a ActorQuery,
        registry: &AttributeRegistry,
        rng: &'a ConditionRng,
    ) -> Option<(Option<&'a AppliedEffects>, EffectExprContext<'a, 'a>)> {
        let Ok((_, source, _)) = actors.get(self.targeting.source()) else {
            warn!(
                "Effect not applied: source {} has no actor view, it is despawned or is not an \
                 actor; fix the EffectTargeting this apply was triggered with",
                self.targeting.source()
            );
            return None;
        };
        let Ok((applied, target, is_actor)) = actors.get(self.targeting.target()) else {
            warn!(
                "Effect not applied: target {} has no actor view, it is despawned or is not an \
                 actor; fix the EffectTargeting this apply was triggered with",
                self.targeting.target()
            );
            return None;
        };
        // an actor's ModifierTable holds the fold even when it carries no attribute of its own
        if !is_actor && !has_any_attribute(&target, registry) {
            warn_once!(
                "Effect {} applied to {} folds into nothing: that entity carries no registered \
                 attribute for the modifiers to land on; an ActorBuilder, or attributes of its \
                 own, would give it one",
                self.handle.id(),
                self.targeting.target()
            );
        }
        Some((
            applied,
            EffectExprContext {
                target_actor: Some(target),
                source_actor: source,
                effect_holder: source,
                rng,
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

    /// read once per apply, ahead of the stack path so a re-application has to pass it too
    fn attach_passes(&self, effect: &EffectDef, context: &EffectExprContext) -> bool {
        effect
            .attach_conditions
            .iter()
            .all(|condition| condition.eval(context))
    }

    /// a stacking policy lands on the copy already applied, so the spawn has nothing left to do
    fn queue_stacked_effect(
        &self,
        effect: &EffectDef,
        applied: Option<&AppliedEffects>,
        effects: &Query<&Effect>,
        add_stack_event: &mut MessageWriter<NotifyAddStackEvent>,
    ) -> Option<Entity> {
        if matches!(effect.stacking_policy, EffectStackingPolicy::None) {
            return None;
        }
        let existing = applied.and_then(|applied| {
            applied.iter().find(|other| {
                effects
                    .get(*other)
                    .is_ok_and(|other| other.0.id() == self.handle.id())
            })
        })?;
        add_stack_event.write(NotifyAddStackEvent {
            effect_entity: existing,
            handle: self.handle,
        });
        Some(existing)
    }

    fn spawn_persistent_effect(
        &self,
        commands: &mut Commands,
        effect: &EffectDef,
        context: &EffectExprContext,
        type_bindings: &AppAttributeBindings,
    ) -> Option<Entity> {
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
            Effect(self.handle),
            EffectTags(effect.granted_tags.iter().map(|tag| tag.type_id).collect()),
        ));
        // the def is immutable once applied, so the marker cannot go stale on a live effect
        if !effect.blocked_by_tags.is_empty() || !effect.activate_conditions.is_empty() {
            effect_commands.insert(EffectConditional);
        }
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
                effect_entity,
                context,
                &bindings,
                effect.application_policy.is_periodic(),
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
    attributes: Res<'w, AttributeRegistry>,
    rng: Res<'w, ConditionRng>,
}

pub(crate) fn apply_effect_event_observer(
    trigger: On<ApplyEffectEvent>,
    actors: ActorQuery,
    ctx: ApplyContext<'_, '_>,
    mut writer: MessageWriter<NotifyAddStackEvent>,
    mut commands: Commands,
) {
    let Some(effect) = ctx.effect_assets.get(&trigger.handle) else {
        warn!(
            "Effect {:?} not applied: the handle holds no EffectDef in Assets<EffectDef>; \
             add the asset before applying it",
            trigger.handle
        );
        return;
    };
    let Some((applied, context)) = trigger.context(&actors, &ctx.attributes, &ctx.rng) else {
        return;
    };
    let source = trigger.targeting.source();
    let target = trigger.targeting.target();

    if !trigger.passes_tag_rules(effect, &context) {
        commands.trigger(EffectBlocked {
            entity: target,
            source,
            handle: trigger.handle,
        });
        return;
    }

    if effect.application_policy == EffectApplicationPolicy::Instant {
        if !effect
            .activate_conditions
            .iter()
            .all(|condition| condition.eval(&context))
        {
            return;
        }
        trigger.cleanse(effect, applied, &ctx.effect_tags, &mut commands);
        let bindings = ctx
            .type_bindings
            .internal
            .read()
            .unwrap_or_else(|err| err.into_inner());
        for modifier in &effect.modifiers {
            modifier.apply_delayed(
                source,
                target,
                trigger.entity,
                &context,
                &bindings,
                &mut commands,
            );
        }
        commands.trigger(EffectApplied {
            entity: target,
            source,
            effect: None,
            handle: trigger.handle,
        });
        return;
    }

    if !trigger.attach_passes(effect, &context) {
        return;
    }

    if let Some(existing) = trigger.queue_stacked_effect(effect, applied, &ctx.effects, &mut writer)
    {
        trigger.cleanse(effect, applied, &ctx.effect_tags, &mut commands);
        commands.trigger(EffectApplied {
            entity: target,
            source,
            effect: Some(existing),
            handle: trigger.handle,
        });
        return;
    }

    if let Some(effect_entity) =
        trigger.spawn_persistent_effect(&mut commands, effect, &context, &ctx.type_bindings)
    {
        trigger.cleanse(effect, applied, &ctx.effect_tags, &mut commands);
        commands.trigger(MarkActorDirty {
            entity: target,
            marks: DirtyMarks::Effect(effect_entity),
        });
        commands.trigger(EffectApplied {
            entity: target,
            source,
            effect: Some(effect_entity),
            handle: trigger.handle,
        });
    }
}
