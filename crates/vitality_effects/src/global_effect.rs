use crate::definition::EffectDef;
use crate::graph::NodeType;
use crate::{ApplyEffectEvent, EffectTargeting};
use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use core::marker::PhantomData;
use vitality_core::attributes::ManageAttributes;
use vitality_core::store::Handle;

#[derive(Resource, Deref, DerefMut, Default)]
pub struct GlobalEffects(Vec<Handle<EffectDef>>);

#[derive(Component, Clone, Copy)]
pub struct GlobalActor;

#[derive(Component, Clone, Copy)]
pub struct GlobalEffect;

/// applies every [`GlobalEffects`] handle to each entity gaining marker `M`; the default stays actor-only
pub struct GlobalEffectPlugin<M = ManageAttributes>(PhantomData<M>);

impl<M: Component> GlobalEffectPlugin<M> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

// Default on the default marker only, so a bare `GlobalEffectPlugin::default()` is not left to inference
impl Default for GlobalEffectPlugin<ManageAttributes> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

// one global actor and one handle list serve every marker instance, so they are installed once
struct SharedInstall;

impl<M: Component> Plugin for GlobalEffectPlugin<M> {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        if vitality_core::first_install::<SharedInstall>(app) {
            app.add_systems(PreStartup, spawn_global_actor);
            app.insert_resource(GlobalEffects::default());
        }

        app.add_observer(observe_spawned_marker::<M>);
    }
}

fn spawn_global_actor(mut commands: Commands) {
    commands.spawn((
        GlobalActor,
        ManageAttributes,
        NodeType::Actor,
        Name::new("Global Effect Actor"),
    ));
}

fn observe_spawned_marker<M: Component>(
    trigger: On<Add<M>>,
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
            handle: *handle,
        });
    }
}
