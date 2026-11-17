use bevy::prelude::*;

/// "Something about this actor's effects changed and no attribute of it did", so an actor can be dirtied
/// from code that is not typed on an attribute. [`crate::resolver`] answers by queueing every
/// attribute the actor has.
#[derive(EntityEvent)]
pub(crate) struct MarkActorDirty {
    pub entity: Entity,
}
