use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use vitality::ability::tasks::{AbilityTask, Complete, DebugInstantTask, DebugLongTask, TaskItem, TaskParam, TaskStatus, Tasks, task, NoData, wait_task};
use vitality::ability::{Abilities, AbilityBuilder, ExecuteAbility, TargetData};
use vitality::actors::ActorBuilder;
use vitality::context::Vitality;
use vitality::inspector::ActorInspectorPlugin;
use vitality::inspector::debug_overlay::DebugOverlayMarker;
use vitality::prelude::*;
use vitality::registry::RegistryMut;
use vitality::registry::ability_registry::AbilityToken;
use vitality::{AttributesPlugin, init_attribute};

pub const FIREBALL: AbilityToken = AbilityToken::new_static("fireball");

attribute!(Health, f32);
attribute!(Damage, f32);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(LogPlugin {
            filter: "error,vitality=debug".into(),
            level: bevy::log::Level::DEBUG,
            ..default()
        }))
        .add_plugins((
            AttributesPlugin,
            init_attribute::<Health>,
            init_attribute::<Damage>,
        ))
        .add_plugins(ActorInspectorPlugin)
        .add_systems(Startup, setup_camera)
        .add_systems(Startup, (setup_ability, setup_actor).chain())
        .add_systems(PreUpdate, inputs)
        .add_systems(Update, print_ability)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d::default());
}

fn setup_ability(mut registry: RegistryMut) {
    let fireball = AbilityBuilder::new()
        .with_name("Fireball".into())
        .with_cooldown(1.0)
        .add_execution(
            |trigger: On<ExecuteAbility>, source: Query<&Health>, _ctx: Vitality| {
                if let Ok(health) = source.get(trigger.source) {
                    println!(
                        "Fireball! {}: {}: H: {}",
                        trigger.ability,
                        trigger.source,
                        health.current_value()
                    );
                }
            },
        )
        .set_tasks(|| {
            bsn! {
                Complete::Any
                Tasks [
                    #LongTask
                    task::<DebugLongTask>(NoData),
                    #WaitTask
                    wait_task(3.0)
                    Tasks [
                        (
                            #SpawnFireball
                            task::<DebugInstantTask>(NoData)
                        ),
                        (
                            #Teleport
                            task::<DebugInstantTask>(NoData)
                        ),
                    ],
                ]
            }
        })
        .build();

    registry.add_ability(FIREBALL, fireball);
}

#[derive(Component, Clone, Debug)]
struct Player;

fn setup_actor(mut vitality: Vitality) {
    let actor = ActorBuilder::new()
        .name("Actor".into())
        .with::<Health>(10.0)
        .with::<Damage>(2.0)
        .clamp::<Health>(0.0, 1.0)
        .insert((DebugOverlayMarker, Player))
        .build();

    let entity = vitality.add_spawn_actor(actor).id();
    vitality
        .grant_ability_by_token_unchecked(entity, &FIREBALL)
        .expect("Failed to grant ability");

    let effect = EffectBuilder::permanent()
        .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
        .build();
    vitality.apply_dynamic_effect_to_self(entity, effect);
}

#[allow(unused)]
struct TestAbilityTask;
impl AbilityTask for TestAbilityTask {
    type EntityItem = TaskContext;
    type SystemParam = TaskSystemParam<'static, 'static>;
    type Data = NoData;

    fn activate(
        _task_id: Entity,
        query: TaskItem<Self>,
        _param: &mut TaskParam<Self>,
    ) -> TaskStatus {
        println!("[{}] Began AbilityTask", query.entity);

        TaskStatus::Running
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct TaskContext {
    entity: Entity,
    health: Read<Health>,
    damage: Read<Damage>,
}

#[derive(SystemParam)]
pub struct TaskSystemParam<'w, 's> {
    _commands: Commands<'w, 's>,
    _time: Res<'w, Time>,
}

fn inputs(
    player: Single<Entity, With<Player>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut abilities: Abilities,
) {
    if keys.just_pressed(KeyCode::KeyQ) {
        abilities.try_activate_by_token(*player, &FIREBALL, TargetData::SelfCast);
    }
}

fn print_ability() {}
