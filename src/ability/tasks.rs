use crate::ability::ability_state::{AbilityEvent, AbilityMachine};
use crate::ability::task_states::{TaskEvent, TaskMachine, TaskState};
use crate::ability::{Abilities, Ability, AbilityOf};
use bevy::ecs::query::{QueryData, QueryItem};
use bevy::ecs::relationship::Relationship;
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

pub type CasterItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::CasterItem>;
pub type AbilityItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::AbilityItem>;
pub type TaskItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::TaskItem>;
pub type TaskParam<'w, 's, T> = SystemParamItem<'w, 's, <T as AbilityTask>::SystemParam>;

pub trait AbilityTask: Send + Sync + 'static {
    /// The query descriptor. (e.g. `&'static mut Health` or a struct deriving `QueryData`)
    type CasterItem: QueryData + Send + Sync + 'static;
    type AbilityItem: QueryData + Send + Sync + 'static;
    type TaskItem: QueryData + Send + Sync + 'static;
    type SystemParam: SystemParam + Send + Sync + 'static;
    type Data: Component + Clone + Send + Sync + 'static;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        _task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        TaskStatus::Complete
    }

    fn on_stop(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        _task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
    }
    fn on_completion(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        _task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
    }
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
    abilities: Query<Entity, With<Ability>>,
    tasks: Query<&TaskOwner>,
    casters: Query<&AbilityOf>,
    mut ability_items: Query<T::AbilityItem>,
    mut task_items: Query<T::TaskItem>,
    mut caster_items: Query<T::CasterItem>,
    params: StaticSystemParam<T::SystemParam>,
    mut commands: Commands,
) {
    let ability_id = find_ability_ancestor(trigger.task_id, &tasks, &abilities);
    let task_item = task_items.get_mut(trigger.task_id).unwrap();
    let ability_item = ability_items.get_mut(ability_id).expect(&format!(
        "[{}] AbilityTask error fetching AbilityItem",
        ability_id
    ));
    let caster_id = casters.get(ability_id).expect(&format!(
        "[{}] Abilities must have an owner caster",
        ability_id
    ));
    let caster_item = caster_items.get_mut(caster_id.0).unwrap();

    let mut param_items = params.into_inner();
    let status = T::activate(
        trigger.task_id,
        caster_item,
        ability_item,
        task_item,
        &mut param_items,
    );

    if status == TaskStatus::Complete {
        commands.trigger(MachineEvent {
            entity: trigger.task_id,
            event: TaskEvent::Complete,
        });
    }
}

fn on_task_completed_observer<T: AbilityTask>(
    trigger: On<TaskCompleted>,
    abilities: Query<Entity, With<Ability>>,
    tasks: Query<&TaskOwner>,
    casters: Query<&AbilityOf>,
    mut ability_items: Query<T::AbilityItem>,
    mut task_items: Query<T::TaskItem>,
    mut caster_items: Query<T::CasterItem>,
    params: StaticSystemParam<T::SystemParam>,
) {
    let ability_id = find_ability_ancestor(trigger.task_id, &tasks, &abilities);
    let task_item = task_items.get_mut(trigger.task_id).unwrap();
    let ability_item = ability_items.get_mut(ability_id).expect(&format!(
        "[{}] AbilityTask error fetching AbilityItem",
        ability_id
    ));
    let caster_id = casters.get(ability_id).expect(&format!(
        "[{}] Abilities must have an owner caster",
        ability_id
    ));
    let caster_item = caster_items.get_mut(caster_id.0).unwrap();

    let mut param_items = params.into_inner();

    T::on_completion(
        trigger.task_id,
        caster_item,
        ability_item,
        task_item,
        &mut param_items,
    );
}

fn on_task_stopped_observer<T: AbilityTask>(
    trigger: On<TaskStopped>,
    abilities: Query<Entity, With<Ability>>,
    tasks: Query<&TaskOwner>,
    casters: Query<&AbilityOf>,
    mut ability_items: Query<T::AbilityItem>,
    mut task_items: Query<T::TaskItem>,
    mut caster_items: Query<T::CasterItem>,
    params: StaticSystemParam<T::SystemParam>,
) {
    let ability_id = find_ability_ancestor(trigger.task_id, &tasks, &abilities);
    let task_item = task_items.get_mut(trigger.task_id).unwrap();
    let ability_item = ability_items.get_mut(ability_id).expect(&format!(
        "[{}] AbilityTask error fetching AbilityItem",
        ability_id
    ));
    let caster_id = casters.get(ability_id).expect(&format!(
        "[{}] Abilities must have an owner caster",
        ability_id
    ));
    let caster_item = caster_items.get_mut(caster_id.0).unwrap();

    let mut param_items = params.into_inner();

    T::on_stop(
        trigger.task_id,
        caster_item,
        ability_item,
        task_item,
        &mut param_items,
    );
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

    debug!(
        "[{}] Task Completion Notification ({:?}) [{result}] ({rule:?})",
        trigger.entity, tasks
    );

    if !abilities.contains(trigger.entity) {
        trigger.propagate(result);
    } else if result {
        let _ = ability_machines.dispatch_event(trigger.entity, AbilityEvent::EndAbility);
    }
}

fn find_ability_ancestor(
    current_entity: Entity,
    parent_query: &Query<&TaskOwner>,
    ability_query: &Query<Entity, With<Ability>>,
) -> Entity {
    let mut current = current_entity;

    // Traverse upwards using the Parent chain
    while let Ok(task_parent) = parent_query.get(current) {
        let parent_entity = task_parent.get();

        // Check if the current parent has the 'Ability' component
        if ability_query.contains(parent_entity) {
            return parent_entity;
        }

        // Move to the next parent
        current = parent_entity;
    }

    unreachable!("should always have an ability in the hierarchy");
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
    type CasterItem = ();
    type AbilityItem = ();
    type TaskItem = DebugTaskContext;
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        debug!("[{}] Activate Task", task.name);

        TaskStatus::Complete
    }

    fn on_stop(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
        debug!("[{}] Task Stopped", task.name);
    }

    fn on_completion(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
        debug!("[{}] Task Completed", task.name);
    }
}

pub struct DebugLongTask;
impl AbilityTask for DebugLongTask {
    type CasterItem = ();
    type AbilityItem = ();
    type TaskItem = DebugTaskContext;
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        debug!("[{}] Activate Task", task.name);

        TaskStatus::Running
    }

    fn on_stop(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
        debug!("[{}] Task Stopped", task.name);
    }

    fn on_completion(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
        debug!("[{}] Task Completed", task.name);
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
    type CasterItem = ();
    type AbilityItem = ();
    type TaskItem = WaitTaskContext;
    type SystemParam = ();
    type Data = WaitTask;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        mut task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        task.timer.0.reset();
        task.timer.0.unpause();
        TaskStatus::Running
    }
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
            abilities.task(task_id).complete();
        }
    }
}

pub struct TaskScope<'a, 'w, 's> {
    task_id: Option<Entity>,
    commands: &'a mut Commands<'w, 's>,
}

impl<'a, 'w, 's> TaskScope<'a, 'w, 's> {
    pub fn new(task: Entity, commands: &'a mut Commands<'w, 's>) -> Self {
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
