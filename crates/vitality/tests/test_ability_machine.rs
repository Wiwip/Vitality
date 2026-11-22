//! Drives the ability and task machines headless: MinimalPlugins only, `AttributesPlugin` installs both.

use std::time::Duration;

#[path = "support/logs.rs"]
mod logs;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::scene::{QueuedScenes, ScenePatch, ScenePlugin};
use bevy::time::TimeUpdateStrategy;
use hfsm_bevy::{
    Machine, MachineDefinition, MachineEvent, MachineExternalContext, MachineLocalContext,
    MachineQuery, StateEntered, StateId, StateTimer,
};
use vitality::AttributesPlugin;
use vitality::ability::AbilityDef;
use vitality::ability::AbilityEnded;
use vitality::ability::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use vitality::ability::task_states::TaskEvent;
use vitality::ability::task_states::{TaskMachine, TaskState};
use vitality::ability::tasks::{
    AbilityItem, AbilityTask, Cast, CasterItem, ChildSettled, Composite, ExecuteTask, NoData,
    OnFail, Settled, Task, TaskFailed, TaskItem, TaskParam, TaskStatus, TaskStopped, Tasks,
    parallel, race, sequence, task, wait_task,
};
use vitality::ability::{Abilities, AbilityBuilder, TargetData};
use vitality::actors::ActorDef;
use vitality::actors::{Actor, ActorBuilder};
use vitality::context::Vitality;
use vitality::effect::GlobalActor;
use vitality::registry::RegistryMut;
use vitality::registry::ability_registry::AbilityToken;

const TEST_ABILITY: AbilityToken = AbilityToken::new_static("headless_test_ability");
/// every clock figure below is derived from this cooldown
const COOLDOWN: f32 = 0.25;
/// the wait gate of the nested trees, short enough to run out in a few fixed steps
const WAIT: f32 = 0.05;

struct InstantTask;

impl AbilityTask for InstantTask {
    type CasterItem = ();
    type AbilityItem = ();
    type TaskItem = ();
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        _task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        TaskStatus::Complete
    }
}

struct FailingTask;

impl AbilityTask for FailingTask {
    type CasterItem = ();
    type AbilityItem = ();
    type TaskItem = ();
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        _task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        TaskStatus::Failed
    }
}

struct LongTask;

impl AbilityTask for LongTask {
    type CasterItem = ();
    type AbilityItem = ();
    type TaskItem = ();
    type SystemParam = ();
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        _caster: CasterItem<Self>,
        _ability: AbilityItem<Self>,
        _task: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        TaskStatus::Running
    }
}

#[derive(Component, Clone)]
struct TestActor;

#[derive(Resource, Default)]
struct Entered(Vec<StateId>);

#[derive(Resource, Default)]
struct EndRequests(usize);

#[derive(Resource, Default)]
struct Failures(Vec<Entity>);

#[derive(Resource, Default)]
struct Starts(Vec<Entity>);

#[derive(Resource, Default)]
struct Stops(Vec<Entity>);

#[derive(Resource, Default)]
struct CompleteRequests(Vec<Entity>);

#[derive(Resource, Default)]
struct Endings(Vec<(bool, bool)>);

fn record_entry(trigger: On<StateEntered<AbilityMachine>>, mut entered: ResMut<Entered>) {
    entered.0.push(trigger.state);
}

fn record_end(trigger: On<MachineEvent<AbilityMachine>>, mut ends: ResMut<EndRequests>) {
    if matches!(trigger.event, AbilityEvent::EndAbility) {
        ends.0 += 1;
    }
}

fn record_failure(trigger: On<TaskFailed>, mut failures: ResMut<Failures>) {
    failures.0.push(trigger.task_id);
}

fn record_start(trigger: On<ExecuteTask>, mut starts: ResMut<Starts>) {
    starts.0.push(trigger.task_id);
}

fn record_stop(trigger: On<TaskStopped>, mut stops: ResMut<Stops>) {
    stops.0.push(trigger.task_id);
}

fn record_complete_request(
    trigger: On<MachineEvent<TaskMachine>>,
    mut requests: ResMut<CompleteRequests>,
) {
    if matches!(trigger.event, TaskEvent::Complete) {
        requests.0.push(trigger.entity);
    }
}

fn record_ending(trigger: On<AbilityEnded>, mut endings: ResMut<Endings>) {
    endings.0.push((trigger.cancelled, trigger.failed));
}

/// Handed to the `Startup` system in a resource: a system closure may not consume what it captures.
#[derive(Resource)]
struct HeldAbility(Option<AbilityDef>);

fn register_ability(mut registry: RegistryMut, mut held: ResMut<HeldAbility>) {
    let ability = held.0.take().expect("the ability is registered once");
    registry.add_ability(TEST_ABILITY, ability);
}

fn spawn_actor(mut ctx: Vitality) {
    ctx.add_spawn_actor(
        ActorBuilder::new()
            .name("Headless")
            .insert(TestActor)
            .build(),
    );
}

fn test_app<S: Scene + 'static, F: Fn() -> S + Send + Sync + 'static>(tasks: F) -> App {
    let ability = AbilityBuilder::new()
        .with_name("Headless".into())
        .with_cooldown(COOLDOWN)
        .set_tasks(tasks)
        .build();

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.insert_resource(Entered::default());
    app.insert_resource(EndRequests::default());
    app.insert_resource(Failures::default());
    app.insert_resource(Starts::default());
    app.insert_resource(Stops::default());
    app.insert_resource(CompleteRequests::default());
    app.insert_resource(Endings::default());
    app.add_observer(record_failure);
    app.add_observer(record_start);
    app.add_observer(record_stop);
    app.add_observer(record_complete_request);
    app.add_observer(record_ending);
    app.insert_resource(HeldAbility(Some(ability)));
    app.add_observer(record_entry);
    app.add_observer(record_end);
    app.add_systems(Startup, (register_ability, spawn_actor));
    app
}

fn spawned_actor(app: &mut App) -> Entity {
    let mut query = app.world_mut().query_filtered::<Entity, With<TestActor>>();
    query.single(app.world()).unwrap()
}

fn grant(app: &mut App) -> Entity {
    let owner = spawned_actor(app);
    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| {
            let handle = abilities.get_ability_from_token(&TEST_ABILITY);
            abilities.grant_ability(&handle, owner)
        })
        .unwrap()
        .unwrap()
}

fn try_activate(app: &mut App, owner: Entity) {
    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| {
            abilities.try_activate_by_token(owner, &TEST_ABILITY, TargetData::SelfCast)
        })
        .unwrap();
}

fn in_state<T>(app: &mut App, entity: Entity, state: impl Into<StateId>) -> bool
where
    T: Machine,
    T::Local: MachineLocalContext,
    T::External: MachineExternalContext,
{
    let state: StateId = state.into();
    app.world_mut()
        .run_system_once(move |machines: MachineQuery<T>| {
            machines.is_in_state(entity, state).unwrap()
        })
        .unwrap()
}

fn cooldown_armed(app: &App, ability: Entity) -> bool {
    app.world()
        .get::<StateTimer<AbilityMachine>>(ability)
        .expect("MachineInstance<T> requires StateTimer<T>")
        .is_armed(AbilityState::Recovery)
}

fn sub_tasks(app: &App, ability: Entity) -> Vec<Entity> {
    app.world()
        .get::<Tasks>(ability)
        .expect("the ability's task scene should have been applied")
        .iter()
        .collect()
}

fn split_completed(app: &mut App, ability: Entity) -> (Option<Entity>, Option<Entity>) {
    let mut done = None;
    let mut waiting = None;
    for task in sub_tasks(app, ability) {
        if in_state::<TaskMachine>(app, task, TaskState::Completed) {
            done = Some(task);
        } else {
            waiting = Some(task);
        }
    }
    (done, waiting)
}

/// one task that finishes on activation, one that runs until the ability ends
fn mixed_tasks() -> impl Scene + 'static {
    bsn! {
        Tasks [
            @task::<InstantTask>(NoData)
            --
            @task::<LongTask>(NoData)
        ]
    }
}

/// Ready -> Active -> Recovery -> Ready, with the cooldown timer carrying it out of Recovery.
#[test]
fn ability_walks_ready_active_recovery_ready() {
    let mut app = test_app(|| bsn! { Tasks [ @task::<LongTask>(NoData) ] });
    app.update();

    let owner = spawned_actor(&mut app);
    let ability = grant(&mut app);
    app.update();
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Ready),
        "a granted ability starts Ready"
    );

    try_activate(&mut app, owner);
    app.update();
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "TryActivate on a Ready ability with a running task goes Active"
    );

    app.world_mut().trigger(MachineEvent::<AbilityMachine>::new(
        ability,
        AbilityEvent::EndAbility,
    ));
    app.update();
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
    assert!(
        cooldown_armed(&app, ability),
        "entering Recovery arms the cooldown timer"
    );

    // a headless update lasts microseconds, so each iteration gets a fixed delta; the count is the cooldown
    let timestep = app.world().resource::<Time<Fixed>>().timestep();
    let updates = Duration::from_secs_f32(COOLDOWN)
        .div_duration_f64(timestep)
        .floor() as usize;
    assert!(
        updates > 1,
        "{COOLDOWN}s is under two fixed steps, so the loop below would assert nothing at all"
    );

    app.insert_resource(TimeUpdateStrategy::ManualDuration(timestep));
    for step in 1..=updates + 1 {
        app.update();
        if step == updates - 1 {
            assert!(
                in_state::<AbilityMachine>(&mut app, ability, AbilityState::Recovery),
                "a cooldown of {COOLDOWN}s one fixed step short of expiring must not leave Recovery"
            );
        }
    }
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Ready),
        "the cooldown of {COOLDOWN}s expires and the ability is Ready again"
    );
    assert!(
        !cooldown_armed(&app, ability),
        "leaving Recovery drops the cooldown timer"
    );
}

/// The default `Parallel` root ends on the last completion, via ChildSettled -> on_child_settled.
#[test]
fn a_parallel_root_ends_the_ability_on_the_last_task_completion() {
    let mut app = test_app(mixed_tasks);
    app.update();

    let owner = spawned_actor(&mut app);
    let ability = grant(&mut app);
    app.update();
    assert!(
        matches!(
            app.world().get::<Composite>(ability),
            Some(Composite::Parallel)
        ),
        "Ability requires a Composite, Parallel unless the scene picks another"
    );

    try_activate(&mut app, owner);
    app.update();

    let (done, waiting) = split_completed(&mut app, ability);
    assert!(done.is_some(), "the instant task should have completed");
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "one of two tasks is still running, so the Parallel root must stay Active"
    );

    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| {
            abilities.task(waiting.unwrap()).complete()
        })
        .unwrap();
    app.update();

    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Recovery),
        "the last subtask completion ends the ability"
    );
    assert!(cooldown_armed(&app, ability));
}

#[test]
fn machine_definition_is_installed_before_the_first_update() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    // Registered before the grant: the machine starts at the grant's command flush, not in the update.
    app.insert_resource(Entered::default());
    app.add_observer(record_entry);

    assert!(
        app.world()
            .contains_resource::<MachineDefinition<AbilityMachine>>(),
        "the AbilityMachine definition must come from the plugin, not from a system"
    );
    assert!(
        app.world()
            .contains_resource::<MachineDefinition<TaskMachine>>(),
        "the TaskMachine definition must come from the plugin, not from a system"
    );

    app.insert_resource(HeldAbility(Some(
        AbilityBuilder::new()
            .set_tasks(|| bsn! { Tasks [ @task::<LongTask>(NoData) ] })
            .build(),
    )));
    app.world_mut()
        .run_system_once(register_ability)
        .expect("the registry is a resource, so registering needs no update");

    // a stand-in `GlobalActor` covers the `Add<Actor>` observer, which unwraps one only `PreStartup` makes
    let global_actor = app.world_mut().spawn(GlobalActor).id();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<ActorDef>>()
        .add(ActorBuilder::new().name("Pre-update").build());
    app.world_mut().spawn((TestActor, Actor(handle)));
    app.world_mut().despawn(global_actor);

    let ability = grant(&mut app);
    app.update();

    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Ready),
        "an ability granted before the first update must be Ready after it"
    );
    assert_eq!(
        1,
        sub_tasks(&app, ability).len(),
        "that same first update is the grant that must have applied the Tasks scene, so the ability has its one subtask"
    );
    assert_eq!(
        vec![StateId::from(AbilityState::Ready)],
        app.world().resource::<Entered>().0,
        "the pre-update grant must start the machine exactly once, in Ready"
    );
}

/// both scene sites, `apply_scene` and `spawn_scene`, read `Assets<ScenePatch>`
#[test]
fn scene_plugin_is_installed_when_the_host_only_inited_the_asset() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app.init_asset::<ScenePatch>();
    app.add_plugins(AttributesPlugin);

    assert!(
        app.is_plugin_added::<ScenePlugin>(),
        "AttributesPlugin must install ScenePlugin even when Assets<ScenePatch> already exists"
    );
    assert!(
        app.world().contains_resource::<QueuedScenes>(),
        "ScenePlugin::build is the only thing that inserts QueuedScenes"
    );
    assert!(
        app.world().contains_resource::<Assets<ScenePatch>>(),
        "the host's own Assets<ScenePatch> must still be there"
    );

    // A host that already added ScenePlugin (DefaultPlugins does) must not be handed a second one.
    let mut app2 = App::new();
    app2.add_plugins((MinimalPlugins, AssetPlugin::default(), ScenePlugin));
    app2.add_plugins(AttributesPlugin);

    assert!(
        app2.is_plugin_added::<ScenePlugin>(),
        "AttributesPlugin must respect the duplicate-add guard at crates/vitality_core/src/lib.rs:74-76, not add ScenePlugin twice"
    );
}

fn recovery_entries(app: &App) -> usize {
    app.world()
        .resource::<Entered>()
        .0
        .iter()
        .filter(|state| **state == StateId::from(AbilityState::Recovery))
        .count()
}

/// a Race root settles on either one, so both asks find the machine still Active at the flush
#[test]
fn two_settles_in_one_update_enter_recovery_once() {
    let mut app = test_app(|| {
        bsn! {
            Composite::Race
            Tasks [
                @task::<LongTask>(NoData)
                --
                @task::<LongTask>(NoData)
            ]
        }
    });
    app.update();

    let owner = spawned_actor(&mut app);
    let ability = grant(&mut app);
    app.update();
    try_activate(&mut app, owner);
    app.update();
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Active
    ));

    let cast = app.world().get::<Cast>(ability).unwrap().0;
    app.world_mut().resource_mut::<Entered>().0.clear();
    app.world_mut().resource_mut::<EndRequests>().0 = 0;
    {
        let tasks = sub_tasks(&app, ability);
        let mut commands = app.world_mut().commands();
        for child in tasks {
            commands.trigger(ChildSettled {
                parent: ability,
                child,
                outcome: Settled::Completed,
                cast,
            });
        }
    }
    app.update();

    assert_eq!(
        2,
        app.world().resource::<EndRequests>().0,
        "both settles must have asked the ability to end"
    );
    assert_eq!(
        1,
        recovery_entries(&app),
        "two end requests in one update must enter Recovery exactly once"
    );
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
    assert!(cooldown_armed(&app, ability));
}

#[test]
fn activating_a_token_the_actor_never_got_warns_with_the_token_and_the_actor() {
    let logs = logs::Logs::scoped();

    let actor = logs.run(|| {
        let mut app = test_app(|| bsn! { Tasks [ @task::<LongTask>(NoData) ] });
        app.update();
        let actor = spawned_actor(&mut app);
        app.world_mut()
            .run_system_once(move |mut abilities: Abilities| {
                abilities.try_activate_by_token(actor, &TEST_ABILITY, TargetData::SelfCast)
            })
            .unwrap();
        actor
    });

    let warns = logs.warns();
    assert!(
        warns.contains(&TEST_ABILITY.to_string())
            && warns.contains(&actor.to_string())
            && warns.contains("grant_ability"),
        "the dropped activation has to warn at WARN level and name the token, the actor and how \
         to grant it, got: {warns}"
    );
}

/// the entities a `Tasks` relationship holds, empty for a task that owns none
fn children(app: &App, parent: Entity) -> Vec<Entity> {
    app.world()
        .get::<Tasks>(parent)
        .map(|tasks| tasks.iter().collect())
        .unwrap_or_default()
}

/// reads a `#Name` out of the task scene, so every assertion below names the task it is about
fn task_named(app: &mut App, name: &str) -> Entity {
    let mut tasks = app
        .world_mut()
        .query_filtered::<(Entity, &Name), With<Task>>();
    tasks
        .iter(app.world())
        .find(|(_, task_name)| task_name.as_str() == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("the task scene built no task named {name}"))
}

/// the manual clock `ability_walks_ready_active_recovery_ready` drives, run for `secs`
fn wait_it_out(app: &mut App, secs: f32) {
    let timestep = app.world().resource::<Time<Fixed>>().timestep();
    // +1 because the update that installs the strategy spends its delta priming the clock
    let updates = Duration::from_secs_f32(secs)
        .div_duration_f64(timestep)
        .ceil() as usize
        + 1;
    app.insert_resource(TimeUpdateStrategy::ManualDuration(timestep));
    for _ in 0..updates {
        app.update();
    }
}

/// builds the app, grants the ability and activates it once, so each tree test starts mid-cast
fn cast<S: Scene + 'static, F: Fn() -> S + Send + Sync + 'static>(tasks: F) -> (App, Entity) {
    let mut app = test_app(tasks);
    app.update();
    let owner = spawned_actor(&mut app);
    let ability = grant(&mut app);
    app.update();
    try_activate(&mut app, owner);
    app.update();
    (app, ability)
}

fn cancel(app: &mut App, ability: Entity) {
    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| abilities.cancel(ability))
        .unwrap();
    app.update();
}

fn complete(app: &mut App, task: Entity) {
    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| abilities.task(task).complete())
        .unwrap();
    app.update();
}

/// a completed sequence means its subtree is done, so the root never ends on a sibling mid-branch
#[test]
fn parallel_waits_for_the_whole_subtree() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                #Branch
                @sequence()
                Tasks [
                    #Gate
                    @wait_task(WAIT)
                    --
                    #Nested
                    @task::<LongTask>(NoData)
                ]
                --
                #Late
                @wait_task(WAIT * 2.0)
            ]
        }
    });

    wait_it_out(&mut app, WAIT * 3.0);
    let nested = task_named(&mut app, "Nested");
    let node_late = task_named(&mut app, "Late");
    assert!(in_state::<TaskMachine>(
        &mut app,
        node_late,
        TaskState::Completed
    ));
    assert!(
        in_state::<TaskMachine>(&mut app, nested, TaskState::Running),
        "the gate ran out, so the sequence started its second child"
    );
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "Late completed, but the branch is not done while Nested runs"
    );

    complete(&mut app, nested);
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Recovery),
        "Nested completes the branch, the last child the root waits for"
    );
}

#[test]
fn sequence_runs_children_in_order() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                @sequence()
                Tasks [
                    #First
                    @wait_task(WAIT)
                    --
                    #Second
                    @task::<InstantTask>(NoData)
                ]
            ]
        }
    });
    let first = task_named(&mut app, "First");
    let second = task_named(&mut app, "Second");
    assert!(in_state::<TaskMachine>(&mut app, first, TaskState::Running));
    assert!(
        in_state::<TaskMachine>(&mut app, second, TaskState::Pending),
        "the second child waits for the first"
    );

    wait_it_out(&mut app, WAIT);
    assert_eq!(vec![first, second], app.world().resource::<Starts>().0);
    assert!(in_state::<TaskMachine>(
        &mut app,
        second,
        TaskState::Completed
    ));
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
}

/// the old `nested_tasks_run_after_their_parent` intent, now spelled as a `Sequence` of waits
#[test]
fn a_sequence_of_waits_ends_after_the_last_one() {
    let (mut app, ability) = cast(|| {
        bsn! { Tasks [ @sequence() Tasks [ @wait_task(WAIT) -- @wait_task(WAIT) ] ] }
    });
    wait_it_out(&mut app, WAIT * 1.5);
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "only the first wait has run out"
    );
    wait_it_out(&mut app, WAIT);
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
}

/// A composite starts its block as it starts itself, so the whole subtree is live with it
#[test]
fn parallel_starts_every_child_at_once() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                #Branch
                @parallel()
                Tasks [
                    #Leaf
                    @task::<InstantTask>(NoData)
                    --
                    #SubBranch
                    @parallel()
                    Tasks [
                        #GrandLeaf
                        @task::<LongTask>(NoData)
                    ]
                ]
            ]
        }
    });
    let branch = task_named(&mut app, "Branch");
    let sub_branch = task_named(&mut app, "SubBranch");
    let grand_leaf = task_named(&mut app, "GrandLeaf");
    assert_eq!(vec![branch], sub_tasks(&app, ability));
    assert_eq!(vec![grand_leaf], children(&app, sub_branch));

    assert!(in_state::<TaskMachine>(
        &mut app,
        branch,
        TaskState::Running
    ));
    let node_leaf = task_named(&mut app, "Leaf");
    assert!(in_state::<TaskMachine>(
        &mut app,
        node_leaf,
        TaskState::Completed
    ));
    assert!(in_state::<TaskMachine>(
        &mut app,
        sub_branch,
        TaskState::Running
    ));
    assert!(
        in_state::<TaskMachine>(&mut app, grand_leaf, TaskState::Running),
        "two levels down still started in the same update"
    );

    complete(&mut app, grand_leaf);
    assert!(in_state::<TaskMachine>(
        &mut app,
        branch,
        TaskState::Completed
    ));
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
}

#[test]
fn a_race_root_ends_on_the_first_child_and_stops_the_rest() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Composite::Race
            Tasks [
                #Quick
                @wait_task(WAIT)
                --
                #Slow
                @task::<LongTask>(NoData)
            ]
        }
    });
    let slow = task_named(&mut app, "Slow");
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Active
    ));

    wait_it_out(&mut app, WAIT);
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
    assert!(in_state::<TaskMachine>(&mut app, slow, TaskState::Stopped));
    assert_eq!(vec![slow], app.world().resource::<Stops>().0);
}

/// a nested race stops its losers itself, before the ability ends
#[test]
fn race_stops_its_losers() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                #Race
                @race()
                Tasks [
                    #Winner
                    @task::<InstantTask>(NoData)
                    --
                    #Loser
                    @task::<LongTask>(NoData)
                ]
                --
                #Hold
                @task::<LongTask>(NoData)
            ]
        }
    });
    let loser = task_named(&mut app, "Loser");
    let node_race = task_named(&mut app, "Race");
    assert!(in_state::<TaskMachine>(
        &mut app,
        node_race,
        TaskState::Completed
    ));
    assert!(in_state::<TaskMachine>(&mut app, loser, TaskState::Stopped));
    assert_eq!(vec![loser], app.world().resource::<Stops>().0);
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "Hold still runs, so the root is not done"
    );
}

#[test]
fn a_leaf_with_children_warns_and_never_runs_them() {
    let logs = logs::Logs::scoped();
    let (leaf, orphan_pending) = logs.run(|| {
        let (mut app, _) = cast(|| {
            bsn! {
                Tasks [
                    #MisusedLeaf
                    @task::<LongTask>(NoData)
                    Tasks [ #Orphan @task::<InstantTask>(NoData) ]
                ]
            }
        });
        let orphan = task_named(&mut app, "Orphan");
        (
            task_named(&mut app, "MisusedLeaf"),
            in_state::<TaskMachine>(&mut app, orphan, TaskState::Pending),
        )
    });
    let warns = logs.warns();
    assert!(orphan_pending, "a leaf never starts the block under it");
    assert_eq!(
        1,
        warns.matches("MisusedLeaf").count(),
        "one warning naming the leaf, got: {warns}"
    );
    assert!(warns.contains(&leaf.to_string()) && warns.contains("sequence()"));
}

#[test]
fn an_empty_composite_completes_at_once() {
    let (mut app, ability) = cast(|| bsn! { Tasks [ @sequence() ] });
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
}

#[test]
fn cancelling_mid_sequence_never_starts_the_rest() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                #Steps
                @sequence()
                Tasks [
                    #Running
                    @task::<LongTask>(NoData)
                    --
                    #Later
                    @task::<InstantTask>(NoData)
                ]
            ]
        }
    });
    let steps = task_named(&mut app, "Steps");
    let running = task_named(&mut app, "Running");
    let later = task_named(&mut app, "Later");

    cancel(&mut app, ability);
    assert!(in_state::<TaskMachine>(
        &mut app,
        running,
        TaskState::Stopped
    ));
    assert!(in_state::<TaskMachine>(&mut app, later, TaskState::Pending));
    let stops = &app.world().resource::<Stops>().0;
    assert_eq!(
        vec![steps, running],
        *stops,
        "the cascade stops the composite, then its running child, and never the pending one"
    );
    assert!(!app.world().resource::<Starts>().0.contains(&later));
}

/// without the pause the stopped wait still ran out and asked a Stopped task to complete
#[test]
fn a_stopped_wait_never_asks_to_complete() {
    let (mut app, ability) = cast(|| bsn! { Tasks [ #Wait @wait_task(WAIT) ] });
    let wait = task_named(&mut app, "Wait");
    cancel(&mut app, ability);
    wait_it_out(&mut app, WAIT * 2.0);
    assert!(
        !app.world().resource::<CompleteRequests>().0.contains(&wait),
        "a stopped wait must not run out"
    );
}

#[test]
fn hold_keeps_the_ability_active_and_fires_task_failed() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                @sequence()
                Tasks [
                    #Broken
                    @task::<FailingTask>(NoData)
                    --
                    #After
                    @task::<InstantTask>(NoData)
                ]
            ]
        }
    });
    let broken = task_named(&mut app, "Broken");
    assert_eq!(vec![broken], app.world().resource::<Failures>().0);
    assert!(
        in_state::<TaskMachine>(&mut app, broken, TaskState::Failed),
        "a task that reports Failed must reach TaskState::Failed instead of staying Running"
    );
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Active
    ));
    let node_after = task_named(&mut app, "After");
    assert!(in_state::<TaskMachine>(
        &mut app,
        node_after,
        TaskState::Pending
    ));
}

#[test]
fn propagate_fails_up_to_the_root_and_ends_the_ability() {
    let (mut app, ability) = cast(|| {
        bsn! {
            OnFail::Propagate
            Tasks [
                #Steps
                @sequence()
                OnFail::Propagate
                Tasks [
                    #Broken
                    @task::<FailingTask>(NoData)
                    --
                    #After
                    @task::<InstantTask>(NoData)
                ]
            ]
        }
    });
    let node_steps = task_named(&mut app, "Steps");
    assert!(in_state::<TaskMachine>(
        &mut app,
        node_steps,
        TaskState::Failed
    ));
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
    assert_eq!(vec![(false, true)], app.world().resource::<Endings>().0);
    let after = task_named(&mut app, "After");
    assert!(!app.world().resource::<Starts>().0.contains(&after));
}

#[test]
fn skip_lets_a_sequence_continue() {
    let (mut app, ability) = cast(|| {
        bsn! {
            Tasks [
                @sequence()
                OnFail::Skip
                Tasks [
                    @task::<FailingTask>(NoData)
                    --
                    #After
                    @task::<InstantTask>(NoData)
                ]
            ]
        }
    });
    let node_after = task_named(&mut app, "After");
    assert!(in_state::<TaskMachine>(
        &mut app,
        node_after,
        TaskState::Completed
    ));
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
    assert_eq!(vec![(false, false)], app.world().resource::<Endings>().0);
}

#[test]
fn a_settle_from_an_earlier_cast_is_ignored() {
    let (mut app, ability) = cast(|| bsn! { Tasks [ #Only @task::<LongTask>(NoData) ] });
    let only = task_named(&mut app, "Only");
    let current = app.world().get::<Cast>(ability).unwrap().0;
    let settle = |cast| ChildSettled {
        parent: ability,
        child: only,
        outcome: Settled::Completed,
        cast,
    };

    app.world_mut().trigger(settle(current.wrapping_sub(1)));
    app.update();
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "a settle stamped with the previous cast must not end this one"
    );

    app.world_mut().trigger(settle(current));
    app.update();
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
}

/// Reset after Recovery lets the same tree run again: the composite's tally starts over.
#[test]
fn a_sequence_runs_again_on_the_next_cast() {
    let (mut app, ability) = cast(|| {
        bsn! { Tasks [ @sequence() Tasks [ #A @task::<InstantTask>(NoData) -- #B @task::<InstantTask>(NoData) ] ] }
    });
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
    wait_it_out(&mut app, COOLDOWN);
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Ready
    ));

    let owner = spawned_actor(&mut app);
    try_activate(&mut app, owner);
    app.update();
    let (a, b) = (task_named(&mut app, "A"), task_named(&mut app, "B"));
    assert_eq!(vec![a, b, a, b], app.world().resource::<Starts>().0);
    assert!(in_state::<AbilityMachine>(
        &mut app,
        ability,
        AbilityState::Recovery
    ));
}
