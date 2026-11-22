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

/// effects changed but no attribute did; the resolver queues the effect's attributes, or all of them
#[derive(EntityEvent)]
pub(crate) struct MarkActorDirty {
    pub entity: Entity,
    pub marks: DirtyMarks,
}
