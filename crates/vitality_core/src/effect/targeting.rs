use bevy::prelude::Entity;

#[derive(Debug)]
pub enum EffectTargeting {
    SelfCast(Entity),
    Targeted { source: Entity, target: Entity },
}

impl EffectTargeting {
    pub fn new(source: Entity, target: Entity) -> Self {
        debug_assert_ne!(
            Entity::PLACEHOLDER,
            source,
            "Source entity cannot be placeholder"
        );
        debug_assert_ne!(
            Entity::PLACEHOLDER,
            target,
            "Target entity cannot be placeholder"
        );

        if source == target {
            Self::SelfCast(source)
        } else {
            Self::Targeted { source, target }
        }
    }

    pub fn source(&self) -> Entity {
        match self {
            Self::SelfCast(entity) | Self::Targeted { source: entity, .. } => *entity,
        }
    }

    pub fn target(&self) -> Entity {
        match self {
            Self::SelfCast(entity) | Self::Targeted { target: entity, .. } => *entity,
        }
    }
}
