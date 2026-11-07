use bevy::prelude::*;

pub type EntityMutatorFn = dyn Fn(&mut EntityCommands) + Send + Sync;

pub struct EntityActions {
    pub func: Box<EntityMutatorFn>,
}

impl EntityActions {
    pub fn new(func: impl Fn(&mut EntityCommands) + Send + Sync + 'static) -> Self {
        Self {
            func: Box::new(func),
        }
    }

    pub fn apply(&self, entity_commands: &mut EntityCommands) {
        (self.func)(entity_commands);
    }
}
