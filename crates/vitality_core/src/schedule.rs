use bevy_ecs::prelude::SystemSet;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectsSet {
    // empty by default
    First,
    Prepare,
    UpdateBaseValues,
    UpdateCurrentValues,
    Notify,
    // empty by default
    Last,
}
