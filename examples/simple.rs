use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::window::PresentMode;
use express_it::plan::Plan;
use std::fmt::Debug;
use std::time::Duration;
use vitality::ability::{AbilityBuilder, ExecuteAbility, TargetData, TryActivateAbility};
use vitality::actors::ActorBuilder;
use vitality::assets::{AbilityDef, EffectDef};
use vitality::attributes::ReflectAccessAttribute;
use vitality::context::{Source, Vitality};
use vitality::effect::{Effect, EffectStackingPolicy};
use vitality::graph::DependencyGraph;
use vitality::inspector::ActorInspectorPlugin;
use vitality::inspector::debug_overlay::DebugOverlayMarker;
use vitality::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::{AttributesPlugin, attribute, init_attribute, tag};

attribute!(Strength, u32);
attribute!(Agility, u32);
attribute!(Intelligence, u32);

attribute!(Health, u32);
attribute!(MaxHealth, u32);
attribute!(HealthRegen, u32);

attribute!(Mana, u32);
attribute!(ManaPool, u32);
attribute!(ManaRegen, u32);

attribute!(AttackPower, u32);
attribute!(Armour, f32);
attribute!(MagicPower, u32);

attribute!(TestU32Attribute, u32);
attribute!(TestU8Attribute, u8);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(LogPlugin {
            filter: "error,vitality=debug".into(),
            level: bevy::log::Level::DEBUG,
            ..default()
        }))
        .add_plugins((
            AttributesPlugin,
            init_attribute::<Strength>,
            init_attribute::<Agility>,
            init_attribute::<Intelligence>,
            init_attribute::<Health>,
            init_attribute::<MaxHealth>,
            init_attribute::<HealthRegen>,
            init_attribute::<Mana>,
            init_attribute::<ManaPool>,
            init_attribute::<ManaRegen>,
            init_attribute::<AttackPower>,
            //init_attribute::<Armour>,
            init_attribute::<MagicPower>,
        ))
        //.add_plugins((EguiPlugin::default(), WorldInspectorPlugin::default()))
        .add_plugins(ActorInspectorPlugin)
        .add_systems(
            Startup,
            (
                setup_effects,
                setup_abilities,
                setup_window,
                setup_actor,
                setup_camera,
            )
                .chain(),
        )
        .add_systems(Update, do_gameplay_stuff)
        .add_systems(PreUpdate, inputs)
        .edit_schedule(Update, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .run();
}

fn setup_window(mut query: Query<&mut Window>) {
    for mut window in query.iter_mut() {
        window.present_mode = PresentMode::Immediate;
    }
}

#[derive(Resource)]
struct AbilityDatabase {
    fireball: Handle<AbilityDef>,
    frostball: Handle<AbilityDef>,
}

#[derive(Resource)]
struct EffectsDatabase {
    ap_buff: Handle<EffectDef>,
    mp_buff: Handle<EffectDef>,
    hp_buff: Handle<EffectDef>,
    hp_regen: Handle<EffectDef>,
}

fn setup_effects(mut commands: Commands, mut ctx: Vitality) {
    // Attack Power effect
    let ap_buff = ctx.add_effect(
        Effect::permanent()
            .name("AttackPower Buff".into())
            .modify::<AttackPower>(Health::src(), ModOp::Add, EffectSubject::Target)
            .modify::<Intelligence>(Health::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );

    // Magic Power effect
    let mp_buff = ctx.add_effect(
        Effect::permanent()
            .name("MagicPower Buff".into())
            .modify::<MagicPower>(Mana::src(), ModOp::Add, EffectSubject::Target)
            .modify::<MagicPower>(5u32, ModOp::Add, EffectSubject::Target)
            //.activate_while(IsAttributeWithinBounds::<Health>::new(..=500, Who::Source))
            /*.with_stacking_policy(EffectStackingPolicy::Add {
                count: 1,
                max_stack: 10,
            })*/
            .build(),
    );

    // Effect 1 - Passive Max Health Boost
    let hp_buff = ctx.add_effect(
        Effect::permanent()
            .name("MaxHealth Increase".into())
            .modify::<MaxHealth>(50u32, ModOp::Add, EffectSubject::Target)
            //.modify::<ManaPool>(Health::source_expr(), ModOp::Add, Who::Target)
            .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
            .build(),
    );

    // Effect 2 - Periodic Health Regen
    let regen = ctx.add_effect(
        Effect::permanent_ticking(1.0)
            .name("Regen".into())
            .modify::<Health>(5, ModOp::Add, EffectSubject::Target)
            .modify::<Mana>(ManaRegen::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );

    /*let global_effect = ctx.add_effect(
        Effect::permanent()
            .name("Global Armour".into())
            .modify::<Armour>(10.0f32, ModOp::Add, Who::Target)
            .build(),
    );*/

    //ctx.add_global_effect(global_effect);

    commands.insert_resource(EffectsDatabase {
        ap_buff,
        mp_buff,
        hp_buff,
        hp_regen: regen,
    });
}

tag!(Fire);
tag!(Frost);

fn setup_abilities(mut effects: ResMut<Assets<AbilityDef>>, mut commands: Commands) {
    let fireball = effects.add(
        AbilityBuilder::new()
            .with_name("Fireball".into())
            .with_cooldown(1.0)
            .with_cost::<Mana>(12)
            .with_tag::<Fire>()
            .add_execution(
                |trigger: On<ExecuteAbility>,
                 source: Query<(&Health, &MaxHealth)>,
                 _ctx: Vitality| {
                    if let Ok((health, _)) = source.get(trigger.source) {
                        println!(
                            "Fireball! {}: {}: H: {}",
                            trigger.ability,
                            trigger.source,
                            health.current_value()
                        );
                    }
                },
            )
            .build(),
    );

    let frostball = effects.add(
        AbilityBuilder::new()
            .with_name("Frostball".into())
            .with_cooldown(1.0)
            .with_cost::<Mana>(10)
            .with_tag::<Frost>()
            .add_execution(
                |trigger: On<ExecuteAbility>,
                 source: Query<(&Health, &MaxHealth)>,
                 _ctx: Vitality| {
                    if let Ok((health, _)) = source.get(trigger.source) {
                        println!(
                            "Frostball! {}: {}: H: {}",
                            trigger.ability,
                            trigger.source,
                            health.current_value()
                        );
                    }
                },
            )
            .on_execute(Plan::new().step(MaxHealth::add::<Source, _>(5u32 + 8u32 + MaxHealth::src())))
            .build(),
    );
    commands.insert_resource(AbilityDatabase {
        fireball,
        frostball,
    });
}

fn setup_actor(mut ctx: Vitality, efx: Res<EffectsDatabase>, abilities: Res<AbilityDatabase>) {
    let actor_template = ActorBuilder::new()
        .name("=== Player ===".into())
        .with::<Strength>(12)
        .with::<Agility>(7)
        .with::<Intelligence>(1)
        // Health
        .with::<Health>(85.0)
        .with::<MaxHealth>(100.0)
        .with::<HealthRegen>(2.0)
        .clamp::<Health>(0, MaxHealth::src())
        // Mana
        .with::<Mana>(30u32)
        .with::<ManaPool>(60.0)
        .with::<ManaRegen>(4.0)
        .clamp::<Mana>(0, ManaPool::src())
        // Misc
        .with::<MagicPower>(1.0)
        .with::<AttackPower>(10.0)
        .with::<Armour>(0.10)
        .insert((Player, DebugOverlayMarker))
        .grant_ability(&abilities.fireball)
        .grant_ability(&abilities.frostball)
        .build();

    let player_entity = ctx.add_spawn_actor(actor_template).id();

    ctx.apply_effect_to_self(player_entity, &efx.ap_buff);
    ctx.apply_effect_to_self(player_entity, &efx.hp_buff);
    ctx.apply_effect_to_self(player_entity, &efx.hp_regen);

    // Should have two stacks
    ctx.apply_effect_to_self(player_entity, &efx.mp_buff);
    ctx.apply_effect_to_self(player_entity, &efx.mp_buff);
}

#[derive(Component, Copy, Clone)]
pub struct Player;

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d::default());
}

fn inputs(
    mut players: Query<(Entity, &AttackPower), With<Player>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    graph: DependencyGraph,
    actors: Query<Entity, With<Player>>,
) {
    if let Ok((player_entity, _)) = players.single_mut() {
        if keys.just_pressed(KeyCode::KeyQ) {
            commands.trigger(TryActivateAbility::by_tag::<Fire>(
                player_entity,
                TargetData::SelfCast,
            ));
        }
        if keys.just_pressed(KeyCode::KeyE) {
            commands.trigger(TryActivateAbility::by_tag::<Frost>(
                player_entity,
                TargetData::SelfCast,
            ));
        }
        if keys.just_pressed(KeyCode::KeyR) {
            analyze_dependencies_with_petgraph(graph, actors);
        }
    }
}

pub fn analyze_dependencies_with_petgraph(
    graph: DependencyGraph,
    actors: Query<Entity, With<Player>>,
) {
    for actor_entity in actors.iter() {
        graph.print_dependencies(actor_entity);
    }
}

fn do_gameplay_stuff() {
    std::thread::sleep(Duration::from_millis(12));
}
