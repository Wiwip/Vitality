use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::{ApplyEffectEvent, EffectTargeting};
use bevy::prelude::*;
use vitality_core::attributes::ManageAttributes;

#[derive(Resource, Deref, DerefMut, Default)]
pub struct GlobalEffects(Vec<Handle<EffectDef>>);

#[derive(Component, Clone, Copy)]
pub struct GlobalActor;

#[derive(Component, Clone, Copy)]
pub struct GlobalEffect;

/// A plugin that manages global effects within the game.
/// Its purpose is to add effects that are applied to any spawned actor.
pub struct GlobalEffectPlugin;

impl Plugin for GlobalEffectPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, spawn_global_actor);
        app.insert_resource(GlobalEffects::default());

        app.add_observer(observe_spawned_actor);
    }
}

pub fn spawn_global_actor(mut commands: Commands) {
    commands.spawn((
        GlobalActor,
        ManageAttributes,
        NodeType::Actor,
        Name::new("Global Effect Actor"),
    ));
}

/// When an actor is spawned, ensures that the global effects apply to the actor.
fn observe_spawned_actor(
    trigger: On<Add<ManageAttributes>>,
    global_actor: Query<Entity, With<GlobalActor>>,
    global_effects: Res<GlobalEffects>,
    mut commands: Commands,
) {
    let Ok(source) = global_actor.single() else {
        return;
    };
    for handle in global_effects.iter() {
        commands.trigger(ApplyEffectEvent {
            entity: trigger.entity,
            targeting: EffectTargeting::new(source, trigger.entity),
            handle: handle.clone(),
        });
    }
}
