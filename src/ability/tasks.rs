use crate::ability::Abilities;
use crate::ability::task_states::{TaskEvent, TaskMachine, TaskState};
use bevy::ecs::query::{QueryData, QueryItem};
use bevy::ecs::system::lifetimeless::Read;
use bevy::ecs::system::{StaticSystemParam, SystemParam, SystemParamItem};
use bevy::prelude::*;
use hfsm_bevy::{MachineInstance, MachineQuery};
use std::cmp::PartialEq;

#[derive(Component, Default, Copy, Clone, Debug)]
pub struct Task;

#[derive(EntityEvent)]
pub struct BeginTask {
    #[event_target]
    pub task_id: Entity,
}

#[derive(EntityEvent)]
pub struct CancelTask {
    #[event_target]
    pub task_id: Entity,
}

#[derive(EntityEvent)]
pub struct EndTask {
    #[event_target]
    pub task_id: Entity,
}

#[derive(EntityEvent)]
pub struct CompleteTask {
    #[event_target]
    pub task_id: Entity,
}

#[derive(Default, Copy, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    #[default]
    Complete,
    Running,
    Failed,
}

pub type TaskItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::Query>;
pub type TaskParam<'w, 's, T> = SystemParamItem<'w, 's, <T as AbilityTask>::Param>;

pub trait AbilityTask: Send + Sync + 'static {
    /// The query descriptor. (e.g. `&'static mut Health` or a struct deriving `QueryData`)
    type Query: QueryData + Send + Sync + 'static;
    type Param: SystemParam + Send + Sync + 'static;
    type Data: Scene + Clone + Send + Sync + 'static;

    fn activate(
        _task_id: Entity,
        _item: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        TaskStatus::Complete
    }

    fn on_cancel(item: TaskItem<Self>) {
        Self::on_end(item);
    }
    fn on_completion(_item: TaskItem<Self>) {}
    fn on_end(_item: TaskItem<Self>) {}
}

pub fn task<T: AbilityTask>(data: T::Data) -> impl Scene {
    bsn! {
        Task
        MachineInstance::<TaskMachine>
        data
        on(begin_task_observer::<T>)
        on(on_cancel_task_observer::<T>)
        on(on_complete_task_observer::<T>)
        on(on_end_task_observer::<T>)
    }
}

fn begin_task_observer<T: AbilityTask>(
    trigger: On<BeginTask>,
    mut query: Query<T::Query>,
    params: StaticSystemParam<T::Param>,
    mut tasks: MachineQuery<TaskMachine>,
) {
    if !tasks.is_in_state(trigger.task_id, TaskState::Pending) {
        error_once!("[{}] The task state is not Pending.", trigger.task_id);
        return;
    }

    tasks
        .dispatch_event(trigger.task_id, TaskEvent::Activate)
        .unwrap();

    let item = query.get_mut(trigger.event_target()).unwrap();
    let mut param_items = params.into_inner();
    let status = T::activate(trigger.task_id, item, &mut param_items);

    if status == TaskStatus::Complete {
        tasks
            .dispatch_event(trigger.task_id, TaskEvent::Complete)
            .unwrap();
    }
}

fn on_cancel_task_observer<T: AbilityTask>(
    trigger: On<CancelTask>,
    mut query: Query<T::Query>,
) {
    let item = query.get_mut(trigger.event_target()).unwrap();
    T::on_cancel(item);
}

fn on_complete_task_observer<T: AbilityTask>(
    trigger: On<CompleteTask>,
    mut query: Query<T::Query>,
) {
    let item = query.get_mut(trigger.event_target()).unwrap();
    T::on_completion(item);
}

fn on_end_task_observer<T: AbilityTask>(trigger: On<EndTask>, mut query: Query<T::Query>) {
    let item = query.get_mut(trigger.event_target()).unwrap();
    T::on_end(item);
}

/// The entity that this effect is targeting.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = Tasks)]
pub struct TaskOwner(pub Entity);

/// All abilities granted to this entity.
#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = TaskOwner, linked_spawn)]
pub struct Tasks(Vec<Entity>);

pub struct DebugTask;
impl AbilityTask for DebugTask {
    type Query = DebugTaskContext;
    type Param = ();
    type Data = ();

    fn activate(
        _task_id: Entity,
        item: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        debug!("[{}] Activate Task", item.name);

        TaskStatus::Complete
    }

    fn on_cancel(item: TaskItem<Self>) {
        debug!("[{}] Task Cancelled", item.name);
    }

    fn on_completion(item: TaskItem<Self>) {
        debug!("[{}] Task Completed", item.name);
    }

    fn on_end(item: TaskItem<Self>) {
        debug!("[{}] Task Ended", item.name);
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct DebugTaskContext {
    entity: Entity,
    name: Read<Name>,
}

#[derive(Component, FromTemplate)]
pub struct WaitTask(Timer);

pub fn handles_wait_task_timers(
    mut tasks: Query<(Entity, &mut WaitTask)>,
    time: Res<Time<Virtual>>,
    mut abilities: Abilities,
) {
    for (task_id, mut wait_task) in tasks.iter_mut() {
        wait_task.0.tick(time.delta());

        /*if wait_task.0.just_finished() {
            abilities.task(task_id).complete();
        }*/
    }
}

pub struct TaskScope<'a, 'w, 's> {
    task_id: Option<Entity>,
    sub_tasks: Vec<Entity>,
    commands: &'a mut Commands<'w, 's>,
}

impl<'a, 'w, 's> TaskScope<'a, 'w, 's> {
    pub fn new(
        task: Entity,
        sub_tasks: impl IntoIterator<Item = Entity>,
        commands: &'a mut Commands<'w, 's>,
    ) -> Self {
        Self {
            task_id: Some(task),
            sub_tasks: sub_tasks.into_iter().collect(),
            commands,
        }
    }

    pub fn empty(commands: &'a mut Commands<'w, 's>) -> Self {
        Self {
            task_id: None,
            sub_tasks: vec![],
            commands,
        }
    }

    pub fn begin(&mut self) {
        let Some(task_id) = self.task_id else {
            return;
        };
        todo!();
        //self.commands.trigger(BeginTask { task_id });
    }

    pub fn complete(&mut self) {
        let Some(task_id) = self.task_id else {
            return;
        };
        todo!();
        self.commands.trigger(CompleteTask { task_id });
    }

    pub fn cancel(&mut self) {
        let Some(task_id) = self.task_id else {
            return;
        };
        todo!();
        self.commands.trigger(CancelTask { task_id })
    }
}
