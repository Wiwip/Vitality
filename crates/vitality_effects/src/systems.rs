use bevy_ecs::prelude::*;

/// effects changed but no attribute did; untyped code can still dirty it, and [`crate::resolver`] queues all
#[derive(EntityEvent)]
pub(crate) struct MarkActorDirty {
    pub entity: Entity,
}
