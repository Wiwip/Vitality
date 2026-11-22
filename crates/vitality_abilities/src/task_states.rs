use crate::tasks::{Cast, ChildSettled, Composite, Settled, StartComposite, TaskOwner};
use crate::tasks::{ExecuteTask, TaskCompleted, TaskFailed, TaskStopped};
use crate::tasks::{Task, Tasks};
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_ecs::resource::IsResource;
use bevy_ecs::system::SystemParam;
use bevy_ecs::system::lifetimeless::Read;
use bevy_log::{debug, warn};
use hfsm_bevy::{Access, MachineDefinition, MachineEvent, MachineState};
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

pub type NodeItem = (
    Option<Read<TaskOwner>>,
    Read<Cast>,
    Has<Composite>,
    Option<Read<Name>>,
);

#[derive(SystemParam)]
pub struct TaskSystemParam<'w, 's> {
    pub tasks: Query<'w, 's, Read<Tasks>, (With<Task>, Without<IsResource>)>,
    pub nodes: Query<'w, 's, NodeItem, (With<Task>, Without<IsResource>)>,
    pub commands: Commands<'w, 's>,
}

impl TaskSystemParam<'_, '_> {
    fn settle(&mut self, task_id: Entity, outcome: Settled) {
        let Ok((Some(owner), cast, _, _)) = self.nodes.get(task_id) else {
            return;
        };
        self.commands.trigger(ChildSettled {
            parent: owner.0,
            child: task_id,
            outcome,
            cast: cast.0,
        });
    }

    /// Pending children ignore Stop, so a stop never reaches a task that did not start.
    fn stop_children(&mut self, task_id: Entity) {
        let Ok(children) = self.tasks.get(task_id) else {
            return;
        };
        for child in children.iter() {
            self.commands
                .trigger(MachineEvent::<TaskMachine>::new(child, TaskEvent::Stop));
        }
    }
}
impl ExternalContext for TaskSystemParam<'static, 'static> {
    type Item<'w, 's> = TaskSystemParam<'w, 's>;
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum TaskEvent {
    Reset,
    Activate,
    Complete,
    Stop,
    Fail,
}

pub(crate) fn build_machine() -> MachineDefinition<TaskMachine> {
    MachineDefinition::<TaskMachine>::builder(TaskState::Pending, |root| {
        root.leaf(TaskState::Pending, "Pending", PendingState)
            .on(TaskEvent::Activate, TaskState::Running);

        root.leaf(TaskState::Running, "Running", RunningState)
            .on(TaskEvent::Complete, TaskState::Completed)
            .on(TaskEvent::Stop, TaskState::Stopped)
            .on(TaskEvent::Fail, TaskState::Failed)
            // fallback when Recovery's Stop could not be dispatched
            .on(TaskEvent::Reset, TaskState::Pending);

        root.leaf(TaskState::Completed, "Completed", CompleteState)
            .on(TaskEvent::Reset, TaskState::Pending);

        root.leaf(TaskState::Stopped, "Cancelled", StoppedState)
            .on(TaskEvent::Reset, TaskState::Pending);

        root.leaf(TaskState::Failed, "Failed", FailedState)
            .on(TaskEvent::Reset, TaskState::Pending);
    })
    .build()
    .expect("Failed to build HFSM")
    .into()
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
        let task_id = ctx.task_id;
        let Ok((_, _, is_composite, name)) = ctx.external.nodes.get(task_id) else {
            return;
        };
        if is_composite {
            ctx.external
                .commands
                .trigger(StartComposite { node: task_id });
            return;
        }
        if ctx
            .external
            .tasks
            .get(task_id)
            .is_ok_and(|children| !children.is_empty())
        {
            let name = name.map(|name| name.as_str()).unwrap_or("Task");
            warn!(
                "[{task_id}] {name} is a leaf task that owns Tasks, so its children never run; \
                 put them under sequence(), parallel() or race()"
            );
        }
        ctx.external.commands.trigger(ExecuteTask { task_id });
    }

    fn on_exit(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_exit: Running Task", ctx.task_id);
    }
}

struct CompleteState;
impl MachineState<TaskMachine> for CompleteState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Complete Task", ctx.task_id);

        ctx.external.commands.trigger(TaskCompleted {
            task_id: ctx.task_id,
        });
        ctx.external.settle(ctx.task_id, Settled::Completed);
    }

    fn on_exit(&self, _ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_exit: Complete Task", _ctx.task_id);
    }
}

struct StoppedState;
impl MachineState<TaskMachine> for StoppedState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Cancelled Task", ctx.task_id);
        ctx.external.commands.trigger(TaskStopped {
            task_id: ctx.task_id,
        });
        ctx.external.stop_children(ctx.task_id);
    }

    fn on_exit(&self, _ctx: &mut Access<TaskMachine>) {}
}

struct FailedState;
impl MachineState<TaskMachine> for FailedState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Failed Task", ctx.task_id);

        ctx.external.commands.trigger(TaskFailed {
            task_id: ctx.task_id,
        });
        ctx.external.stop_children(ctx.task_id);
        ctx.external.settle(ctx.task_id, Settled::Failed);
    }
}
