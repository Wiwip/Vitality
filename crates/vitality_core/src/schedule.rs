use bevy::prelude::SystemSet;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectsSet {
    // Runs first. (empty by default)
    First,
    // Initializes spawned actors and persistent effects
    Prepare,
    // Applies instant modifiers mutating attribute base values
    UpdateBaseValues,
    // Traverse the dirty effects tree and updates current values
    UpdateCurrentValues,
    Notify,
    // Runs last. (empty by default)
    Last,
}
