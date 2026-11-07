use bevy::prelude::*;
use std::time::Duration;

#[derive(Default, Copy, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    #[default]
    Complete,
    Running,
    Failed,
}

#[derive(Component, Default, Copy, Clone, Debug)]
pub struct Task;

#[derive(EntityEvent)]
pub struct ExecuteTask {
    #[event_target]
    pub task_id: Entity,
}

#[derive(EntityEvent)]
pub struct TaskStopped {
    #[event_target]
    pub task_id: Entity,
}

#[derive(EntityEvent)]
pub struct TaskCompleted {
    #[event_target]
    pub task_id: Entity,
}

#[derive(EntityEvent)]
#[entity_event(propagate = &'static TaskOwner, auto_propagate)]
pub struct NotifyTaskCompletion {
    #[event_target]
    pub entity: Entity,
}

/// The entity that this effect is targeting.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = Tasks)]
pub struct TaskOwner(pub Entity);

/// All abilities granted to this entity.
#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = TaskOwner, linked_spawn)]
#[require[Task, Complete::All]]
pub struct Tasks(Vec<Entity>);

#[derive(Component, Debug, FromTemplate)]
pub enum Complete {
    #[default]
    All,
    Any,
}

#[derive(Component, Reflect, Clone, Default)]
#[reflect(Component)]
pub struct NoData;

#[derive(Component, Default, Clone, Reflect)]
pub struct WaitTask(Timer);

impl WaitTask {
    pub fn from_secs(seconds: f32) -> Self {
        let mut timer = Timer::from_seconds(seconds, TimerMode::Once);
        timer.pause();
        Self(timer)
    }

    pub fn from_duration(duration: Duration) -> Self {
        let mut timer = Timer::new(duration, TimerMode::Once);
        timer.pause();
        Self(timer)
    }

    pub fn start(&mut self) {
        self.0.reset();
        self.0.unpause();
    }

    pub fn tick(&mut self, delta: Duration) {
        self.0.tick(delta);
    }

    pub fn just_finished(&self) -> bool {
        self.0.just_finished()
    }
}
