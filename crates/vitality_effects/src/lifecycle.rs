use crate::definition::EffectDef;
use crate::systems::MarkActorDirty;
use crate::{Effect, EffectTarget};
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use vitality_core::store::Assets;
use vitality_core::store::Handle;
use vitality_core::tags::ChangeTag;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect)]
pub enum EffectRemovalReason {
    Expired,
    Removed,
    Cleansed,
    /// the target picked up a `blocked_by_tag` tag, which makes the effect immune while it carries it
    BlockedByTag,
    /// the effect entity went away without going through a removal API, e.g. its source or target despawned
    Despawned,
}

/// Set right before a despawn so the removal observer can report why.
#[derive(Component, Clone, Copy, Debug)]
pub struct PendingRemoval(pub EffectRemovalReason);

#[derive(EntityEvent, Clone, Debug)]
pub struct EffectApplied {
    pub entity: Entity,
    pub source: Entity,
    /// None for instant effects, which never spawn an entity.
    pub effect: Option<Entity>,
    pub handle: Handle<EffectDef>,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct EffectRemoved {
    pub entity: Entity,
    pub effect: Entity,
    pub handle: Handle<EffectDef>,
    pub reason: EffectRemovalReason,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct EffectStacksChanged {
    pub entity: Entity,
    pub effect: Entity,
    pub old: u32,
    pub new: u32,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct EffectBlocked {
    pub entity: Entity,
    pub source: Entity,
    pub handle: Handle<EffectDef>,
}

/// Removes one effect entity, reporting `reason` through [`EffectRemoved`].
pub fn remove_effect(commands: &mut Commands, effect: Entity, reason: EffectRemovalReason) {
    if let Ok(mut entity) = commands.get_entity(effect) {
        entity.try_insert(PendingRemoval(reason));
        entity.try_despawn();
    }
}

pub(crate) fn on_effect_removed(
    trigger: On<Remove<Effect>>,
    effects: Query<(&Effect, Option<&EffectTarget>, Option<&PendingRemoval>)>,
    effect_assets: Res<Assets<EffectDef>>,
    mut commands: Commands,
) {
    let Ok((effect, target, pending)) = effects.get(trigger.entity) else {
        return;
    };
    let Some(target) = target else {
        return;
    };
    let target = target.0;
    let reason = pending.map_or(EffectRemovalReason::Despawned, |pending| pending.0);

    if let Some(def) = effect_assets.get(&effect.0)
        && let Ok(mut actor) = commands.get_entity(target)
    {
        for tag in &def.granted_tags {
            actor.queue_silenced(ChangeTag::revoke(*tag));
        }
    }

    commands.trigger(MarkActorDirty { entity: target });
    commands.trigger(EffectRemoved {
        entity: target,
        effect: trigger.entity,
        handle: effect.0,
        reason,
    });
}
