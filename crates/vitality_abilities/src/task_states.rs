use bevy::prelude::*;
use hfsm_bevy::{Access, MachineDefinition, MachineEvent, MachineState};
use vitality_core::ability::task_states::{TaskEvent, TaskMachine, TaskState};
use vitality_core::ability::tasks::{
    ExecuteTask, NotifyTaskCompletion, TaskCompleted, TaskStopped,
};

pub(crate) fn build_machine() -> MachineDefinition<TaskMachine> {
    MachineDefinition::<TaskMachine>::builder(TaskState::Pending, |root| {
        root.leaf(TaskState::Pending, "Pending", PendingState)
            .on(TaskEvent::Activate, TaskState::Running);

        root.leaf(TaskState::Running, "Running", RunningState)
            .on(TaskEvent::Complete, TaskState::Completed)
            .on(TaskEvent::Stop, TaskState::Stopped)
            .on(TaskEvent::TimeOut, TaskState::Stopped);

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

        // Tells this task to begin
        ctx.external.commands.trigger(ExecuteTask {
            task_id: ctx.task_id,
        });
    }

    fn on_exit(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_exit: Running Task", ctx.task_id);
    }
}

struct CompleteState;
impl MachineState<TaskMachine> for CompleteState {
    fn on_enter(&self, ctx: &mut Access<TaskMachine>) {
        debug!("[{}] on_enter: Complete Task", ctx.task_id);

        // Complete this task
        ctx.external.commands.trigger(TaskCompleted {
            task_id: ctx.task_id,
        });

        // Tells subtasks to activate
        if let Ok(sub_tasks) = ctx.external.tasks.get(ctx.task_id) {
            for task_id in sub_tasks.iter() {
                ctx.external
                    .commands
                    .trigger(MachineEvent::<TaskMachine>::new(
                        task_id,
                        TaskEvent::Activate,
                    ));
            }
        };

        // Notify parents of completion
        ctx.external.commands.trigger(NotifyTaskCompletion {
            entity: ctx.task_id,
        });
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
