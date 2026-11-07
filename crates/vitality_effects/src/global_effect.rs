use bevy::prelude::*;
use vitality_core::actors::{Actor, ActorBuilder};
use vitality_core::context::Vitality;
use vitality_core::effect::GlobalEffects;
use vitality_core::effect::global_effect::GlobalActor;

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

pub fn spawn_global_actor(mut commands: Commands, mut ctx: Vitality) {
    let global_actor = commands.spawn(GlobalActor).id();

    let actor = ActorBuilder::new().name("Global Effect Actor").build();

    let actor_handle = ctx.add_actor(actor);
    ctx.insert_actor(global_actor, &actor_handle);
}

/// When an actor is spawned, ensures that the global effects apply to the actor.
fn observe_spawned_actor(trigger: On<Add<Actor>>, mut ctx: Vitality) {
    ctx.spawn_global_effects(trigger.entity);
}
