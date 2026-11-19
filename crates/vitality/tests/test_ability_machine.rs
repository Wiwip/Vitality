//! Drives the ability and task machines headless: MinimalPlugins only, so no display, bevy_ui or
//! asset files are needed. `AttributesPlugin` brings `AbilityPlugin`, which installs both machines.

use std::time::Duration;

#[path = "support/logs.rs"]
mod logs;

use bevy::ecs::system::RunSystemOnce;
use bevy::log::tracing::subscriber::with_default;
use bevy::log::tracing_subscriber::layer::SubscriberExt;
use bevy::prelude::*;
use bevy::scene::{QueuedScenes, ScenePatch, ScenePlugin};
use bevy::time::TimeUpdateStrategy;
use hfsm_bevy::{
    Machine, MachineDefinition, MachineEvent, MachineExternalContext, MachineLocalContext,
    MachineQuery, StateEntered, StateId, StateTimer,
};
use vitality::AttributesPlugin;
use vitality::ability::AbilityDef;
use vitality::ability::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use vitality::ability::task_states::{TaskMachine, TaskState};
use vitality::ability::tasks::{
    AbilityItem, AbilityTask, CasterItem, Complete, NoData, NotifyTaskCompletion, TaskFailed,
    TaskItem, TaskParam, TaskStatus, Tasks, task,
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
    app.add_observer(record_failure);
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

    // a headless update lasts microseconds, so each iteration is handed a fixed timestep as a manual
    // delta or `StateTimer` would never tick; their count comes from the ability's own cooldown
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

/// `Complete::All` only lets the ability end once every subtask completed, and the last completion
/// is what drives NotifyTaskCompletion -> on_task_completion_notification -> EndAbility.
#[test]
fn complete_all_ends_the_ability_on_the_last_task_completion() {
    let mut app = test_app(mixed_tasks);
    app.update();

    let owner = spawned_actor(&mut app);
    let ability = grant(&mut app);
    app.update();
    assert!(
        matches!(app.world().get::<Complete>(ability), Some(Complete::All)),
        "Tasks requires Complete::All"
    );

    try_activate(&mut app, owner);
    app.update();

    let (done, waiting) = split_completed(&mut app, ability);
    assert!(done.is_some(), "the instant task should have completed");
    assert!(
        in_state::<AbilityMachine>(&mut app, ability, AbilityState::Active),
        "one of two tasks is not Complete::All, so the ability must stay Active"
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

/// `TaskFailed` is where listeners pick the end/retry/ignore policy
#[test]
fn a_failing_task_reaches_failed_and_fires_task_failed() {
    let mut app = test_app(|| bsn! { Tasks [ @task::<FailingTask>(NoData) ] });
    app.update();

    let owner = spawned_actor(&mut app);
    let ability = grant(&mut app);
    app.update();

    try_activate(&mut app, owner);
    app.update();

    let task = sub_tasks(&app, ability)[0];
    assert!(
        in_state::<TaskMachine>(&mut app, task, TaskState::Failed),
        "a task that reports Failed must reach TaskState::Failed instead of staying Running"
    );
    assert_eq!(
        vec![task],
        app.world().resource::<Failures>().0,
        "entering Failed fires TaskFailed once, targeted at the task"
    );
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

    // a stand-in `GlobalActor` covers the `Add<Actor>` observer, which unwraps a global actor that
    // only `PreStartup` creates, and goes away before the update
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

fn two_long_tasks() -> impl Scene + 'static {
    bsn! {
        Tasks [
            @task::<LongTask>(NoData)
            --
            @task::<LongTask>(NoData)
        ]
    }
}

/// both notifications land in one update, so the end fires at the command flush and both find the
/// machine still Active; the library serialises its own cascades, so the second one is queued by hand
/// with `Complete` lifted, else the first completion would end the ability
#[test]
fn two_completion_notifications_enter_recovery_once() {
    let mut app = test_app(two_long_tasks);
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

    app.world_mut().entity_mut(ability).remove::<Complete>();
    for task in sub_tasks(&app, ability) {
        app.world_mut()
            .run_system_once(move |mut abilities: Abilities| abilities.task(task).complete())
            .unwrap();
        app.update();
    }
    app.world_mut().entity_mut(ability).insert(Complete::All);

    let notified = sub_tasks(&app, ability)[0];
    app.world_mut().resource_mut::<Entered>().0.clear();
    app.world_mut().resource_mut::<EndRequests>().0 = 0;
    {
        let mut commands = app.world_mut().commands();
        commands.trigger(NotifyTaskCompletion { entity: notified });
        commands.trigger(NotifyTaskCompletion { entity: notified });
    }
    app.update();

    assert_eq!(
        2,
        app.world().resource::<EndRequests>().0,
        "both notifications must have asked the ability to end"
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
    let logs = logs::Logs::default();
    let subscriber = bevy::log::tracing_subscriber::Registry::default().with(logs.layer());

    let actor = with_default(subscriber, || {
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
