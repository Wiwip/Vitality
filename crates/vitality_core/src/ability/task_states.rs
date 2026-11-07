use crate::ability::tasks::{Task, Tasks};
use bevy::ecs::query::QueryData;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use hfsm_bevy::{ExternalContext, LocalContext, Machine, StateId};

#[derive(Clone)]
pub struct TaskMachine;
impl Machine for TaskMachine {
    type Local = TaskContext;
    type External = TaskSystemParam<'static, 'static>;
    type Event = TaskEvent;
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskState {
    _Root,
    Pending,
    Running,
    Completed,
    Stopped,
    Failed,
}
impl From<TaskState> for StateId {
    fn from(value: TaskState) -> Self {
        Self::try_from(value as u16).unwrap()
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct TaskContext {
    pub task_id: Entity,
}
impl LocalContext for TaskContext {
    type Item<'w, 's> = <Self as QueryData>::Item<'w, 's>;
}

#[derive(SystemParam)]
pub struct TaskSystemParam<'w, 's> {
    pub tasks: Query<'w, 's, Read<Tasks>, (With<Task>, Without<IsResource>)>,
    pub commands: Commands<'w, 's>,
}
impl ExternalContext for TaskSystemParam<'static, 'static> {
    type Item<'w, 's> = TaskSystemParam<'w, 's>;
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum TaskEvent {
    Reset,
    Activate,
    Execute,
    Complete,
    Stop,
    TimeOut,
}
