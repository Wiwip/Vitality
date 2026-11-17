#![allow(dead_code)]
//! Headless harness for the gameplay tests: public API only, one `update` is one fixed tick.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;
use vitality::actors::ActorBuilder;
use vitality::effect::definition::EffectDef;
use vitality::prelude::*;
use vitality::{attribute, tag};

attribute!(Health, f32);
attribute!(MaxHealth, f32);
attribute!(Mana, f32);
attribute!(Power, f32);
attribute!(Armor, f32);

tag!(Burning);
tag!(Stunned);
tag!(Immune);
tag!(Magic);
tag!(Casting);

/// bevy's default fixed timestep, so `ManualDuration(STEP)` gives exactly one fixed tick per update.
pub const STEP: Duration = Duration::from_micros(15625);

pub fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        VitalityPlugin::new()
            .attribute::<Health>()
            .attribute::<MaxHealth>()
            .attribute::<Mana>()
            .attribute::<Power>()
            .attribute::<Armor>(),
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
    // the fixed clock has no tick in the first update
    app.update();
    app
}

pub fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

pub fn ticks_for(seconds: f32) -> usize {
    (seconds / STEP.as_secs_f32()).round() as usize
}

/// Spawns an actor and lets one tick resolve its starting values.
pub fn spawn(app: &mut App, builder: ActorBuilder) -> Entity {
    let mut def = Some(builder.build());
    let entity = app
        .world_mut()
        .run_system_once(move |mut vitality: Vitality| {
            vitality.add_spawn_actor(def.take().expect("run once")).id()
        })
        .unwrap();
    ticks(app, 1);
    entity
}

pub fn effect(app: &mut App, def: EffectDef) -> Handle<EffectDef> {
    app.world_mut().resource_mut::<Assets<EffectDef>>().add(def)
}

/// Runs `f` with the `Effects` param; its commands apply before this returns.
pub fn with_effects<R: Send + 'static>(
    app: &mut App,
    f: impl FnOnce(&mut Effects) -> R + Send + Sync + 'static,
) -> R {
    let mut f = Some(f);
    app.world_mut()
        .run_system_once(move |mut effects: Effects| (f.take().expect("run once"))(&mut effects))
        .unwrap()
}

pub fn with_vitality<R: Send + 'static>(
    app: &mut App,
    f: impl FnOnce(&mut Vitality) -> R + Send + Sync + 'static,
) -> R {
    let mut f = Some(f);
    app.world_mut()
        .run_system_once(move |mut vitality: Vitality| (f.take().expect("run once"))(&mut vitality))
        .unwrap()
}

pub fn with_abilities<R: Send + 'static>(
    app: &mut App,
    f: impl FnOnce(&mut Abilities) -> R + Send + Sync + 'static,
) -> R {
    let mut f = Some(f);
    app.world_mut()
        .run_system_once(move |mut abilities: Abilities| {
            (f.take().expect("run once"))(&mut abilities)
        })
        .unwrap()
}

pub fn apply(app: &mut App, source: Entity, target: Entity, handle: &Handle<EffectDef>) {
    let handle = handle.clone();
    with_effects(app, move |effects| effects.apply(source, target, &handle));
}

pub fn current<T: Attribute>(app: &App, entity: Entity) -> T::Property {
    app.world()
        .get::<T>(entity)
        .expect("actor has the attribute")
        .current_value()
}

pub fn base<T: Attribute>(app: &App, entity: Entity) -> T::Property {
    app.world()
        .get::<T>(entity)
        .expect("actor has the attribute")
        .base_value()
}

pub fn has<C: Component>(app: &App, entity: Entity) -> bool {
    app.world().get::<C>(entity).is_some()
}

#[derive(Resource)]
pub struct Recorded<E>(pub Vec<E>);

/// Starts collecting every `E` triggered from now on.
pub fn record<E: Event + Clone>(app: &mut App) {
    app.insert_resource(Recorded::<E>(Vec::new()));
    app.add_observer(|trigger: On<E>, mut recorded: ResMut<Recorded<E>>| {
        recorded.0.push(trigger.event().clone());
    });
}

pub fn recorded<E: Event + Clone>(app: &App) -> Vec<E> {
    app.world().resource::<Recorded<E>>().0.clone()
}

pub fn hero_def() -> ActorBuilder {
    ActorBuilder::new()
        .name("Hero")
        .with::<Health>(100.0)
        .with::<MaxHealth>(100.0)
        .with::<Mana>(50.0)
        .with::<Power>(10.0)
        .with::<Armor>(0.0)
}
