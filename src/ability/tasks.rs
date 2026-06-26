use crate::ability::ability_state::{AbilityEvent, AbilityMachine};
use crate::ability::task_states::{TaskEvent, TaskMachine, TaskState};
use crate::ability::{Abilities, Ability};
use bevy::ecs::query::{QueryData, QueryItem};
use bevy::ecs::system::lifetimeless::{Read, Write};
use bevy::ecs::system::{StaticSystemParam, SystemParam, SystemParamItem};
use bevy::prelude::*;
use hfsm_bevy::{MachineEvent, MachineInstance, MachineQuery};
use std::cmp::PartialEq;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

#[derive(Default, Copy, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    #[default]
    Complete,
    Running,
    Failed,
}

pub type TaskItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::EntityItem>;
pub type TaskParam<'w, 's, T> = SystemParamItem<'w, 's, <T as AbilityTask>::SystemParam>;

pub trait AbilityTask: Send + Sync + 'static {
    /// The query descriptor. (e.g. `&'static mut Health` or a struct deriving `QueryData`)
    type EntityItem: QueryData + Send + Sync + 'static;
    type SystemParam: SystemParam + Send + Sync + 'static;
    type Data: Component + Clone + Send + Sync + 'static;

    fn activate(
        _task_id: Entity,
        _item: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        TaskStatus::Complete
    }

    fn on_stop(_item: TaskItem<Self>) {}
    fn on_completion(_item: TaskItem<Self>) {}
}

pub fn task<T: AbilityTask>(data: T::Data) -> impl Scene {
    let cell = Arc::new(Mutex::new(Some(data)));
    bsn! {
        Task
        MachineInstance<TaskMachine>

        on({
            let cell = cell.clone();
            move |trigger: On<Add, Task>, mut commands: Commands| {
                if let Ok(mut guard) = cell.lock() {
                    if let Some(payload_data) = guard.take() {
                        commands.entity(trigger.entity).insert(payload_data);
                        commands.entity(trigger.observer()).despawn();
                    }
                }
            }
        })
        on(on_execute_task_observer::<T>)
        on(on_task_completed_observer::<T>)
        on(on_task_stopped_observer::<T>)
    }
}

pub fn wait_task(secs: f32) -> impl Scene {
    task::<WaitTask>(WaitTask::from_secs(secs))
}

fn on_execute_task_observer<T: AbilityTask>(
    trigger: On<ExecuteTask>,
    mut query: Query<T::EntityItem>,
    params: StaticSystemParam<T::SystemParam>,
    mut commands: Commands,
) {
    let item = query.get_mut(trigger.event_target()).unwrap();
    let mut param_items = params.into_inner();
    let status = T::activate(trigger.task_id, item, &mut param_items);

    if status == TaskStatus::Complete {
        commands.trigger(MachineEvent {
            entity: trigger.task_id,
            event: TaskEvent::Complete,
        });
    }
}

fn on_task_completed_observer<T: AbilityTask>(
    trigger: On<TaskCompleted>,
    mut query: Query<T::EntityItem>,
) {
    let item = query.get_mut(trigger.event_target()).unwrap();
    T::on_completion(item);
}

fn on_task_stopped_observer<T: AbilityTask>(
    trigger: On<TaskStopped>,
    mut query: Query<T::EntityItem>,
) {
    let item = query.get_mut(trigger.event_target()).unwrap();
    T::on_stop(item);
}

pub fn on_task_completion_notification(
    mut trigger: On<NotifyTaskCompletion>,
    query: Query<(&Tasks, &Complete)>,
    abilities: Query<&Ability>,
    mut ability_machines: MachineQuery<AbilityMachine>,
) {
    let Ok((tasks, rule)) = query.get(trigger.entity) else {
        // Has no subtasks, not a problem.
        return;
    };
    let task_machines = &ability_machines.view.task_machines;

    let result = match rule {
        Complete::All => tasks
            .iter()
            .all(|task| task_machines.is_in_state(task, TaskState::Completed)),
        Complete::Any => tasks
            .iter()
            .any(|task| task_machines.is_in_state(task, TaskState::Completed)),
    };

    println!(
        "[{}] Task Completion Notification ({:?}) [{result}] ({rule:?})",
        trigger.entity, tasks
    );

    if !abilities.contains(trigger.entity) {
        trigger.propagate(result);
    } else if result {
        let _ = ability_machines.dispatch_event(trigger.entity, AbilityEvent::EndAbility);
    }
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

pub struct DebugInstantTask;
impl AbilityTask for DebugInstantTask {
    type EntityItem = DebugTaskContext;
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        item: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        debug!("[{}] Activate Task", item.name);

        TaskStatus::Complete
    }

    fn on_stop(item: TaskItem<Self>) {
        debug!("[{}] Task Stopped", item.name);
    }

    fn on_completion(item: TaskItem<Self>) {
        debug!("[{}] Task Completed", item.name);
    }
}

pub struct DebugLongTask;
impl AbilityTask for DebugLongTask {
    type EntityItem = DebugTaskContext;
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        item: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        debug!("[{}] Activate Task", item.name);

        TaskStatus::Running
    }

    fn on_stop(item: TaskItem<Self>) {
        debug!("[{}] Task Stopped", item.name);
    }

    fn on_completion(item: TaskItem<Self>) {
        debug!("[{}] Task Completed", item.name);
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct DebugTaskContext {
    entity: Entity,
    name: Read<Name>,
}

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
}

impl AbilityTask for WaitTask {
    type EntityItem = WaitTaskContext;
    type SystemParam = ();
    type Data = WaitTask;

    fn activate(
        _task_id: Entity,
        mut item: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        item.timer.0.reset();
        item.timer.0.unpause();

        println!("timer reset");

        TaskStatus::Running
    }

    fn on_stop(_item: TaskItem<Self>) {}

    fn on_completion(_item: TaskItem<Self>) {}
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct WaitTaskContext {
    entity: Entity,
    timer: Write<WaitTask>,
}

pub fn handles_wait_task_timers(
    mut tasks: Query<(Entity, &mut WaitTask)>,
    time: Res<Time<Virtual>>,
    mut abilities: Abilities,
) {
    for (task_id, mut wait_task) in tasks.iter_mut() {
        wait_task.0.tick(time.delta());

        if wait_task.0.just_finished() {
            println!("timer just finished");
            abilities.task(task_id).complete();
        }
    }
}

pub struct TaskScope<'a, 'w, 's> {
    task_id: Option<Entity>,
    commands: &'a mut Commands<'w, 's>,
}

impl<'a, 'w, 's> TaskScope<'a, 'w, 's> {
    pub fn new(
        task: Entity,
        commands: &'a mut Commands<'w, 's>,
    ) -> Self {
        Self {
            task_id: Some(task),
            commands,
        }
    }

    pub fn empty(commands: &'a mut Commands<'w, 's>) -> Self {
        Self {
            task_id: None,
            commands,
        }
    }

    pub fn execute(&mut self) {
        let Some(task_id) = self.task_id else {
            return;
        };
        self.commands.trigger(MachineEvent {
            entity: task_id,
            event: TaskEvent::Execute,
        });
    }

    pub fn complete(&mut self) {
        let Some(task_id) = self.task_id else {
            return;
        };
        self.commands.trigger(MachineEvent {
            entity: task_id,
            event: TaskEvent::Complete,
        });
    }

    pub fn stop(&mut self) {
        let Some(task_id) = self.task_id else {
            return;
        };
        self.commands.trigger(MachineEvent {
            entity: task_id,
            event: TaskEvent::Stop,
        });
    }
}
