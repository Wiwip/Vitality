use bevy_ecs::prelude::*;
use vitality_core::registry::AttrSet;

/// Which attributes of [`MarkActorDirty::entity`] the fold has to do again.
#[derive(Clone, Copy)]
pub(crate) enum DirtyMarks {
    /// the effect is in the world by the time the trigger lands, so its modifiers name the attributes
    Effect(Entity),
    /// the modifiers are gone by then, so the caller read the names off them first
    Attrs(AttrSet),
    /// nothing could name them
    Everything,
}

/// effects changed but no attribute did; untyped code can still dirty it, and [`crate::resolver`]
/// queues the attributes the changed effect names, or every attribute under [`DirtyMarks::Everything`]
#[derive(EntityEvent)]
pub(crate) struct MarkActorDirty {
    pub entity: Entity,
    pub marks: DirtyMarks,
}
