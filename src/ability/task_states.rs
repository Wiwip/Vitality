use crate::ability::tasks::{BeginTask, CancelTask, CompleteTask, Task, Tasks};
use bevy::ecs::query::QueryData;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use hfsm_bevy::{
    Access, ExternalContext, LocalContext, Machine, MachineDefinition, MachineState, StateId,
};

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
    Cancelled,
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
    task_id: Entity,
    //timers: Write<StateTimer<TaskMachine>>,
}
impl LocalContext for TaskContext {
    type Item<'w, 's> = <Self as QueryData>::Item<'w, 's>;
}

#[derive(SystemParam)]
pub struct TaskSystemParam<'w, 's> {
    tasks: Query<'w, 's, Read<Tasks>, (With<Task>, Without<IsResource>)>,
    commands: Commands<'w, 's>,
}
impl ExternalContext for TaskSystemParam<'static, 'static> {
    type Item<'w, 's> = TaskSystemParam<'w, 's>;
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum TaskEvent {
    Reset,
    Activate,
    ExecuteTask,
    Complete,
    Cancel,
    TimeOut,
}

fn build_machine() -> MachineDefinition<TaskMachine> {
    MachineDefinition::<TaskMachine>::builder(TaskState::Pending, |root| {
        root.leaf(TaskState::Pending, "Pending", PendingState)
            .on(TaskEvent::Activate, TaskState::Running);

        root.leaf(TaskState::Running, "Running", RunningState)
            .on(TaskEvent::Complete, TaskState::Completed)
            .on(TaskEvent::Cancel, TaskState::Cancelled)
            .on(TaskEvent::TimeOut, TaskState::Cancelled)
            .then(TaskState::Completed);

        root.leaf(TaskState::Completed, "Completed", CompleteState)
            .then(TaskState::Pending);

        root.leaf(TaskState::Cancelled, "Cancelled", CancelledState)
            .on(TaskEvent::Reset, TaskState::Pending)
            .then(TaskState::Pending);

        root.leaf(TaskState::Failed, "Failed", FailedState)
            .on(TaskEvent::Reset, TaskState::Pending);
    })
    .build()
    .expect("Failed to build HFSM")
    .into()
}

pub fn setup_task_machine_definition(mut commands: Commands) {
    commands.insert_resource(build_machine());
}

struct PendingState;
impl MachineState<TaskMachine> for PendingState {
    fn on_enter(&self, _ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Pending Task", _ctx.task_id);
    }

    fn on_exit(&self, _ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_exit: Pending Task", _ctx.task_id);
    }
}

struct RunningState;
impl MachineState<TaskMachine> for RunningState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Running Task", ctx.task_id);
    }

    fn on_exit(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_exit: Running Task", ctx.task_id);

        let Ok(sub_tasks) = ctx.view.tasks.get(ctx.task_id) else {
            return;
        };

        for task_id in sub_tasks.iter() {
            ctx.view.commands.trigger(BeginTask { task_id })
        }
    }
}

struct CompleteState;
impl MachineState<TaskMachine> for CompleteState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Complete Task", ctx.task_id);
        ctx.view.commands.trigger(CompleteTask {
            task_id: ctx.task_id,
        });
    }

    fn on_exit(&self, _ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_exit: Complete Task", _ctx.task_id);
    }
}

struct CancelledState;
impl MachineState<TaskMachine> for CancelledState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        ctx.view.commands.trigger(CancelTask {
            task_id: ctx.task_id,
        });
        ctx.internal_events.push_back(TaskEvent::Reset);
    }

    fn on_exit(&self, _ctx: &mut Access<TaskMachine>) {}
}

struct FailedState;
impl MachineState<TaskMachine> for FailedState {
    fn on_enter(&self, _ctx: &mut Access<TaskMachine>) {}

    fn on_exit(&self, _ctx: &mut Access<TaskMachine>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_mermaid_state_machine() {
        let machine = build_machine();
        println!("{}", machine.to_mermaid().unwrap());
    }
}
