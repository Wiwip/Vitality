use bevy_ecs::prelude::*;

/// something about an actor's effects changed and no attribute of it did, so code that is not typed on an attribute can still dirty it; [`crate::resolver`] answers by queueing every attribute the actor has
#[derive(EntityEvent)]
pub(crate) struct MarkActorDirty {
    pub entity: Entity,
}
