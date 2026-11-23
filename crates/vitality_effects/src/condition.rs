use crate::EffectsRef;
use crate::conditions::ConditionRng;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::lifecycle::{EffectRemovalReason, remove_effect};
use crate::systems::{DirtyMarks, MarkActorDirty};
use crate::{Effect, EffectConditional, EffectInactive, EffectSource, EffectTarget};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::relationship::Relationship;
use tracing::debug;
use tracing::error;
use vitality_core::schedule::EffectsSet;
use vitality_core::store::Assets;
use vitality_core::view::ActorRead;

pub struct ConditionPlugin;

impl Plugin for ConditionPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        app.init_resource::<ConditionRng>().add_systems(
            FixedUpdate,
            evaluate_effect_conditions.in_set(EffectsSet::Prepare),
        );
    }
}

/// one effect with the actors it reads its conditions from
type ConditionRow<'w, 's> = (
    EffectsRef<'w, 's>,
    &'w Effect,
    &'w EffectSource,
    &'w EffectTarget,
    Option<&'w EffectInactive>,
);

/// the marker says the walk switched the effect off, and only the walk can clear it again
type WatchedEffects = Or<(With<EffectConditional>, With<EffectInactive>)>;

/// the walk stays on one thread: a row is cheap to decide and the task hand-off costs more than it wins
fn evaluate_effect_conditions(
    query: Query<ConditionRow<'_, '_>, WatchedEffects>,
    carriers: Query<(Entity, &Effect, Option<&EffectConditional>)>,
    parents: Query<EffectsRef>,
    effects: Res<Assets<EffectDef>>,
    rng: Res<ConditionRng>,
    mut commands: Commands,
) {
    // one draw a tick, forked per row so rolls don't depend on thread count
    let base = rng.next_u64();
    if effects.take_revised() {
        derive_markers(&carriers, &effects, &mut commands);
    }
    let decisions = decide_serially(&query, &parents, &effects, base);

    for decision in decisions {
        apply(&mut commands, decision);
    }
}

/// the spawn decided the marker off a def that `get_mut` may have rewritten since, so re-read them all
fn derive_markers(
    carriers: &Query<(Entity, &Effect, Option<&EffectConditional>)>,
    effects: &Assets<EffectDef>,
    commands: &mut Commands,
) {
    for (entity, effect, marker) in carriers.iter() {
        let Some(effect) = effects.get(&effect.0) else {
            continue;
        };
        let watched = !effect.blocked_by_tags.is_empty() || !effect.activate_conditions.is_empty();
        if watched == marker.is_some() {
            continue;
        }
        if watched {
            commands.entity(entity).insert(EffectConditional);
        } else {
            commands.entity(entity).remove::<EffectConditional>();
        }
    }
}

struct Decision {
    effect: Entity,
    target: Entity,
    verdict: Verdict,
}

enum Verdict {
    Blocked,
    BecameActive,
    BecameInactive,
}

fn apply(commands: &mut Commands, decision: Decision) {
    let Decision {
        effect,
        target,
        verdict,
    } = decision;
    match verdict {
        Verdict::Blocked => remove_effect(commands, effect, EffectRemovalReason::BlockedByTag),
        Verdict::BecameActive => {
            debug!("Effect {effect} is now active.");
            commands.entity(effect).remove::<EffectInactive>();
            // nothing else dirties the target: no attribute changed and no leftover mark carries the fold
            commands.trigger(MarkActorDirty {
                entity: target,
                marks: DirtyMarks::Effect(effect),
            });
            // an `Effect` modifier folds onto the effect entity, and the target's mark cannot reach it
            commands.trigger(MarkActorDirty {
                entity: effect,
                marks: DirtyMarks::Own(effect),
            });
        }
        Verdict::BecameInactive => {
            debug!("Effect {effect} is now inactive.");
            commands.entity(effect).insert(EffectInactive);
            commands.trigger(MarkActorDirty {
                entity: target,
                marks: DirtyMarks::Effect(effect),
            });
            commands.trigger(MarkActorDirty {
                entity: effect,
                marks: DirtyMarks::Own(effect),
            });
        }
    }
}

fn decide(
    row: &ConditionRow<'_, '_>,
    parents: &Query<EffectsRef>,
    effects: &Assets<EffectDef>,
    rng: &ConditionRng,
) -> Option<Decision> {
    let (effect_entity_ref, effect, source, target, status) = *row;
    let effect_entity = effect_entity_ref.id();
    let Some(effect) = effects.get(&effect.0) else {
        error!("Effect {effect_entity} has no effect definition.");
        return None;
    };

    // nothing can flip without a tag rule, a condition or the marker, so this row owes no actor lookup
    if effect.blocked_by_tags.is_empty()
        && effect.activate_conditions.is_empty()
        && status.is_none()
    {
        return None;
    }

    let Ok(target_actor_ref) = parents.get(target.0) else {
        error!(
            "Effect {} has no target entity {}.",
            effect_entity, target.0
        );
        return None;
    };

    // immunity is a standing promise, not an apply-time gate, so the tag is re-read every tick
    if effect
        .blocked_by_tags
        .iter()
        .any(|tag| target_actor_ref.has_type(tag.type_id))
    {
        return Some(Decision {
            effect: effect_entity,
            target: target.0,
            verdict: Verdict::Blocked,
        });
    }

    // this system is the only writer of EffectInactive, so no conditions and no marker means no-op
    if effect.activate_conditions.is_empty() && status.is_none() {
        return None;
    }

    let Ok(source_actor_ref) = parents.get(source.get()) else {
        error!(
            "Effect {} has no parent entity {}.",
            effect_entity,
            source.get()
        );
        return None;
    };

    let context = EffectExprContext {
        target_actor: Some(target_actor_ref),
        source_actor: source_actor_ref,
        effect_holder: effect_entity_ref,
        rng,
    };

    let should_be_active = effect
        .activate_conditions
        .iter()
        .all(|condition| condition.eval(&context));

    let verdict = match (should_be_active, status.is_some()) {
        (true, true) => Verdict::BecameActive,
        (false, false) => Verdict::BecameInactive,
        _ => return None,
    };
    Some(Decision {
        effect: effect_entity,
        target: target.0,
        verdict,
    })
}

fn decide_serially(
    query: &Query<ConditionRow<'_, '_>, WatchedEffects>,
    parents: &Query<EffectsRef>,
    effects: &Assets<EffectDef>,
    base: u64,
) -> Vec<Decision> {
    query
        .iter()
        .enumerate()
        .filter_map(|(i, row)| decide(&row, parents, effects, &ConditionRng::fork(base, i as u64)))
        .collect()
}
