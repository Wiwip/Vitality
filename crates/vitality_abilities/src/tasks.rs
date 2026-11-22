use crate::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use crate::system_param::Abilities;
use crate::task_states::{TaskEvent, TaskMachine, TaskState};
use crate::{Ability, AbilityOf};
use bevy_ecs::prelude::*;
use bevy_ecs::query::{QueryData, QueryItem};
use bevy_ecs::relationship::Relationship;
use bevy_ecs::system::lifetimeless::{Read, Write};
use bevy_ecs::system::{StaticSystemParam, SystemParam, SystemParamItem};
use bevy_log::{debug, error, warn};
use bevy_reflect::prelude::Reflect;
use bevy_scene::{Scene, bsn, on};
use bevy_time::{Time, Timer, TimerMode, Virtual};
use hfsm_bevy::{MachineDefinition, MachineEvent, MachineInstance, MachineQuery};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default, Copy, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    #[default]
    Complete,
    Running,
    Failed,
}

#[derive(Component, Default, Copy, Clone, Debug)]
#[require(Cast)]
pub struct Task;

/// How a node with `Tasks` runs and judges its children; the ability itself is the root composite.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, FromTemplate)]
#[require(OnFail, Progress, Cast)]
pub enum Composite {
    /// Starts every child and completes once all of them completed.
    #[default]
    Parallel,
    /// Starts one child at a time and completes after the last one.
    Sequence,
    /// Starts every child, completes on the first completion and stops the rest.
    Race,
}

/// What a composite does when one of its children fails.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, FromTemplate)]
pub enum OnFail {
    /// `TaskFailed` fires and the composite keeps running; listeners decide (D-101).
    #[default]
    Hold,
    /// The composite fails too, stopping its other children; at the root the ability ends failed.
    Propagate,
    /// The failed child counts as finished.
    Skip,
}

/// A composite's tally for the current cast, reset whenever it starts.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Progress {
    done: usize,
    failed: usize,
    next: usize,
}

/// Which cast of its ability a node was started in, so a late settle from an earlier cast is dropped.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cast(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    Completed,
    Failed,
}

/// Sent to a node's owner when the node enters `Completed` or `Failed`.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct ChildSettled {
    #[event_target]
    pub parent: Entity,
    pub child: Entity,
    pub outcome: Settled,
    pub cast: u32,
}

/// Starts a composite's children; triggered when a composite enters Running or an ability goes Active.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub(crate) struct StartComposite {
    #[event_target]
    pub node: Entity,
}

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
pub struct TaskFailed {
    #[event_target]
    pub task_id: Entity,
}

#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = Tasks)]
pub struct TaskOwner(pub Entity);

#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = TaskOwner, linked_spawn)]
pub struct Tasks(Vec<Entity>);

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

    pub fn pause(&mut self) {
        self.0.pause();
    }
}

pub type CasterItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::CasterItem>;
pub type AbilityItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::AbilityItem>;
pub type TaskItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::TaskItem>;
pub type TaskParam<'w, 's, T> = SystemParamItem<'w, 's, <T as AbilityTask>::SystemParam>;

pub trait AbilityTask: Send + Sync + 'static {
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
            move |trigger: On<Add<Task>>, mut commands: Commands| {
                if let Ok(mut guard) = cell.lock()
                    && let Some(payload_data) = guard.take()
                {
                    commands.entity(trigger.entity).insert(payload_data);
                    commands.entity(trigger.observer()).despawn();
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

pub fn sequence() -> impl Scene {
    bsn! { Task MachineInstance<TaskMachine> Composite::Sequence }
}

pub fn parallel() -> impl Scene {
    bsn! { Task MachineInstance<TaskMachine> Composite::Parallel }
}

pub fn race() -> impl Scene {
    bsn! { Task MachineInstance<TaskMachine> Composite::Race }
}

#[derive(SystemParam)]
struct TaskContext<'w, 's, T: AbilityTask> {
    abilities: Query<'w, 's, Entity, With<Ability>>,
    tasks: Query<'w, 's, &'static TaskOwner>,
    casters: Query<'w, 's, &'static AbilityOf>,
    ability_items: Query<'w, 's, <T as AbilityTask>::AbilityItem>,
    task_items: Query<'w, 's, <T as AbilityTask>::TaskItem>,
    caster_items: Query<'w, 's, <T as AbilityTask>::CasterItem>,
    params: StaticSystemParam<'w, 's, <T as AbilityTask>::SystemParam>,
}

impl<'w, 's, T: AbilityTask> TaskContext<'w, 's, T> {
    /// Resolves the task's ability ancestor and caster, then hands every item to `run`.
    fn with_items<F>(self, task_id: Entity, run: F)
    where
        F: FnOnce(
            Entity,
            CasterItem<'w, 's, T>,
            AbilityItem<'w, 's, T>,
            TaskItem<'w, 's, T>,
            &mut TaskParam<'w, 's, T>,
        ),
    {
        let Some(ability_id) = find_ability_ancestor(task_id, &self.tasks, &self.abilities) else {
            warn!("[{}] Task has no ability above it.", task_id);
            return;
        };
        let Ok(caster_id) = self.casters.get(ability_id) else {
            warn!("[{ability_id}] Ability has no caster.");
            return;
        };
        // get_inner, not get_mut: the items must outlive this borrow of `self` to reach `run`
        let (Ok(task_item), Ok(ability_item), Ok(caster_item)) = (
            self.task_items.get_inner(task_id),
            self.ability_items.get_inner(ability_id),
            self.caster_items.get_inner(caster_id.0),
        ) else {
            warn!("[{}] Task query items don't match their entities.", task_id);
            return;
        };

        let mut param_items = self.params.into_inner();
        run(
            task_id,
            caster_item,
            ability_item,
            task_item,
            &mut param_items,
        );
    }
}

fn on_execute_task_observer<T: AbilityTask>(
    trigger: On<ExecuteTask>,
    ctx: TaskContext<'_, '_, T>,
    mut commands: Commands,
) {
    ctx.with_items(trigger.task_id, |task_id, caster, ability, task, param| {
        let status = T::activate(task_id, caster, ability, task, param);

        if status == TaskStatus::Complete {
            commands.trigger(MachineEvent::<TaskMachine>::new(
                task_id,
                TaskEvent::Complete,
            ));
        } else if status == TaskStatus::Failed {
            commands.trigger(MachineEvent::<TaskMachine>::new(task_id, TaskEvent::Fail));
        }
    });
}

fn on_task_completed_observer<T: AbilityTask>(
    trigger: On<TaskCompleted>,
    ctx: TaskContext<'_, '_, T>,
) {
    ctx.with_items(trigger.task_id, |task_id, caster, ability, task, param| {
        T::on_completion(task_id, caster, ability, task, param);
    });
}

fn on_task_stopped_observer<T: AbilityTask>(trigger: On<TaskStopped>, ctx: TaskContext<'_, '_, T>) {
    ctx.with_items(trigger.task_id, |task_id, caster, ability, task, param| {
        T::on_stop(task_id, caster, ability, task, param);
    });
}

fn start_child(commands: &mut Commands, child: Entity, cast: u32) {
    // the insert lands before the Activate, so the child's settle carries this cast
    commands.entity(child).insert(Cast(cast));
    commands.trigger(MachineEvent::<TaskMachine>::new(child, TaskEvent::Activate));
}

/// Ends the node as `outcome`: an ability through its own machine, a composite through `TaskMachine`.
fn settle_node(commands: &mut Commands, node: Entity, is_ability: bool, outcome: Settled) {
    // a trigger, not a dispatch: one system cannot borrow both machine queries
    match (is_ability, outcome) {
        (true, Settled::Completed) => commands.trigger(MachineEvent::<AbilityMachine>::new(
            node,
            AbilityEvent::EndAbility,
        )),
        (true, Settled::Failed) => commands.trigger(MachineEvent::<AbilityMachine>::new(
            node,
            AbilityEvent::FailAbility,
        )),
        (false, Settled::Completed) => {
            commands.trigger(MachineEvent::<TaskMachine>::new(node, TaskEvent::Complete))
        }
        (false, Settled::Failed) => {
            commands.trigger(MachineEvent::<TaskMachine>::new(node, TaskEvent::Fail))
        }
    }
}

/// Starts children synchronously, so every sibling is Running before any of them can settle.
pub(crate) fn on_start_composite(
    trigger: On<StartComposite>,
    mut composites: Query<(&Composite, Option<&Tasks>, &mut Progress, Has<Ability>)>,
    mut access: ParamSet<(Query<&mut Cast>, MachineQuery<TaskMachine>)>,
    mut commands: Commands,
) {
    let node = trigger.node;
    let Ok((kind, tasks, mut progress, is_ability)) = composites.get_mut(node) else {
        return;
    };
    *progress = Progress::default();
    let children: Vec<Entity> = tasks
        .map(|tasks| tasks.iter().collect())
        .unwrap_or_default();
    let starting = match kind {
        Composite::Sequence => children.len().min(1),
        Composite::Parallel | Composite::Race => children.len(),
    };
    progress.next = starting;

    {
        let mut casts = access.p0();
        let Ok(mut cast) = casts.get_mut(node) else {
            return;
        };
        if is_ability {
            cast.0 = cast.0.wrapping_add(1);
        }
        let cast = cast.0;
        for child in &children[..starting] {
            if let Ok(mut child_cast) = casts.get_mut(*child) {
                child_cast.0 = cast;
            }
        }
    }

    if children.is_empty() {
        settle_node(&mut commands, node, is_ability, Settled::Completed);
        return;
    }
    let mut machines = access.p1();
    for child in &children[..starting] {
        if let Err(err) = machines.dispatch_event(*child, TaskEvent::Activate) {
            error!("[{child}] TaskMachine dispatch failed: {err}");
        }
    }
}

type JudgeItem = (
    &'static Composite,
    &'static OnFail,
    Option<&'static Tasks>,
    &'static Cast,
    &'static mut Progress,
    Has<Ability>,
);

/// The only place a composite judges its children; it reads its own tally, not their states.
pub(crate) fn on_child_settled(
    trigger: On<ChildSettled>,
    // Cast is read only: the task machine query below reads it too
    mut nodes: Query<JudgeItem>,
    task_machines: MachineQuery<TaskMachine>,
    // MachineQuery<AbilityMachine> would pull AbilitySystemParam, which owns a MachineQuery<TaskMachine>
    ability_states: Query<&'static MachineInstance<AbilityMachine>>,
    ability_defs: Res<MachineDefinition<AbilityMachine>>,
    mut commands: Commands,
) {
    let ChildSettled {
        parent,
        child,
        outcome,
        cast: settled_cast,
    } = *trigger.event();
    let Ok((kind, on_fail, tasks, cast, mut progress, is_ability)) = nodes.get_mut(parent) else {
        return;
    };
    if settled_cast != cast.0 {
        debug!("[{parent}] dropped a settle of {child} from cast {settled_cast}");
        return;
    }
    // an ability that left Active ignores EndAbility anyway; a composite that stopped must not move
    if !is_ability
        && !task_machines
            .is_in_state(parent, TaskState::Running)
            .inspect_err(|err| error!("[{parent}] TaskMachine is_in_state failed: {err}"))
            .unwrap_or_default()
    {
        return;
    }
    let children: Vec<Entity> = tasks
        .map(|tasks| tasks.iter().collect())
        .unwrap_or_default();

    let counts = match (outcome, on_fail) {
        (Settled::Completed, _) => {
            progress.done += 1;
            true
        }
        (Settled::Failed, OnFail::Hold) => return,
        (Settled::Failed, OnFail::Propagate) => {
            settle_node(&mut commands, parent, is_ability, Settled::Failed);
            return;
        }
        (Settled::Failed, OnFail::Skip) => {
            progress.failed += 1;
            false
        }
    };

    match kind {
        Composite::Sequence => {
            // the check above leaves the ability to its own machine: an ended root starts no child
            let active = ability_states
                .get(parent)
                .is_ok_and(|instance| ability_defs.is_in_state(instance, AbilityState::Active));
            if is_ability && !active {
                return;
            }
            if let Some(next) = children.get(progress.next).copied() {
                progress.next += 1;
                start_child(&mut commands, next, cast.0);
            } else {
                settle_node(&mut commands, parent, is_ability, Settled::Completed);
            }
        }
        Composite::Parallel => {
            if progress.done + progress.failed >= children.len() {
                settle_node(&mut commands, parent, is_ability, Settled::Completed);
            }
        }
        Composite::Race if counts => {
            for other in children.into_iter().filter(|other| *other != child) {
                let running = task_machines
                    .is_in_state(other, TaskState::Running)
                    .inspect_err(|err| error!("[{other}] TaskMachine is_in_state failed: {err}"))
                    .unwrap_or_default();
                if running {
                    commands.trigger(MachineEvent::<TaskMachine>::new(other, TaskEvent::Stop));
                }
            }
            settle_node(&mut commands, parent, is_ability, Settled::Completed);
        }
        Composite::Race => {
            // a skipped failure only loses the race once nobody is left to win it
            if progress.failed >= children.len() {
                settle_node(&mut commands, parent, is_ability, Settled::Failed);
            }
        }
    }
}

fn find_ability_ancestor(
    current_entity: Entity,
    parent_query: &Query<&TaskOwner>,
    ability_query: &Query<Entity, With<Ability>>,
) -> Option<Entity> {
    let mut current = current_entity;

    while let Ok(task_parent) = parent_query.get(current) {
        let parent_entity = task_parent.get();

        if ability_query.contains(parent_entity) {
            return Some(parent_entity);
        }

        current = parent_entity;
    }

    None
}

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
        task.timer.start();
        TaskStatus::Running
    }

    fn on_stop(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        mut task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) {
        // a stopped wait must not run out later and complete a task that is no longer running
        task.timer.pause();
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
        wait_task.tick(time.delta());

        if wait_task.just_finished() {
            abilities.task(task_id).complete();
        }
    }
}

pub struct TaskScope<'a, 'w, 's> {
    task_id: Entity,
    commands: &'a mut Commands<'w, 's>,
}

impl<'a, 'w, 's> TaskScope<'a, 'w, 's> {
    pub fn new(task: Entity, commands: &'a mut Commands<'w, 's>) -> Self {
        Self {
            task_id: task,
            commands,
        }
    }

    pub fn complete(&mut self) {
        self.commands.trigger(MachineEvent::<TaskMachine>::new(
            self.task_id,
            TaskEvent::Complete,
        ));
    }
}
