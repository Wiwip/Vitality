use crate::AbilityRef;
use crate::ability_state::{AbilityMachine, AbilityState};
use crate::context::AbilityExprContext;
use crate::definition::AbilityDef;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use hfsm_bevy::StateTimer;
use std::any::TypeId;
use std::time::Duration;
use vitality_core::context::ActorExprContext;
use vitality_core::view::ActorRead;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationError {
    OnCooldown,
    AlreadyActive,
    CannotAfford,
    /// A tag on the caster, or an active ability's block, forbids it.
    Blocked,
    ConditionFailed,
    NotGranted,
    /// The `TargetData::Target` entity has no actor view anymore.
    NoSuchTarget,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct AbilityActivated {
    /// The ability entity.
    pub entity: Entity,
    pub caster: Entity,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct AbilityActivationFailed {
    /// The ability entity.
    pub entity: Entity,
    pub caster: Entity,
    pub reason: ActivationError,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct AbilityEnded {
    /// The ability entity.
    pub entity: Entity,
    pub caster: Entity,
    pub cancelled: bool,
}

/// Whether the active run is ending through a cancel, read when the ability leaves Active.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct AbilityEnding {
    pub cancelled: bool,
}

/// Ability tags blocked on this caster by its active abilities, counted.
#[derive(Component, Default, Debug)]
pub struct BlockedAbilityTags(pub(crate) HashMap<TypeId, u32>);

impl BlockedAbilityTags {
    pub fn blocks(&self, type_id: TypeId) -> bool {
        self.0.get(&type_id).is_some_and(|count| *count > 0)
    }
}

pub(crate) fn change_blocks(caster: &mut EntityCommands, tags: Vec<TypeId>, add: bool) {
    if tags.is_empty() {
        return;
    }
    caster.queue_silenced(move |mut entity: EntityWorldMut| {
        let mut blocks = entity.take::<BlockedAbilityTags>().unwrap_or_default();
        for tag in &tags {
            let count = blocks.0.entry(*tag).or_default();
            if add {
                *count += 1;
            } else {
                *count = count.saturating_sub(1);
            }
        }
        entity.insert(blocks);
    });
}

/// Everything except the machine's own state: conditions, tags and cost.
pub(crate) fn check_activation(
    def: &AbilityDef,
    caster: &AbilityRef,
    ability: &AbilityRef,
    target: Option<AbilityRef>,
) -> Result<(), ActivationError> {
    let blocked = caster.get::<BlockedAbilityTags>();
    if def
        .tags
        .iter()
        .any(|tag| blocked.is_some_and(|blocked| blocked.blocks(tag.type_id)))
        || def
            .activation_blocked_by
            .iter()
            .any(|tag| caster.has_type(tag.type_id))
        || !def
            .activation_requires
            .iter()
            .all(|tag| caster.has_type(tag.type_id))
    {
        return Err(ActivationError::Blocked);
    }

    let actor_context = ActorExprContext {
        actor_context: caster.into(),
    };
    if !def
        .execution_conditions
        .iter()
        .all(|expr| expr.eval(&actor_context))
    {
        return Err(ActivationError::ConditionFailed);
    }

    let context = AbilityExprContext {
        caster_ref: *caster,
        ability_ref: *ability,
        target_ref: target,
    };
    if !def.cost_condition.iter().all(|expr| expr.eval(&context)) {
        return Err(ActivationError::CannotAfford);
    }
    Ok(())
}

/// Cooldown left on an ability, None while it isn't recovering.
pub fn cooldown_remaining(timer: &StateTimer<AbilityMachine>) -> Option<Duration> {
    timer
        .get(AbilityState::Recovery)
        .map(|(timer, _)| timer.remaining())
}
