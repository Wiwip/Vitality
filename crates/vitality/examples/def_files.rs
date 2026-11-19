//! An effect and an ability whose declared half lives in `assets/defs`, so the numbers are edited
//! as data. Run it from the repo root, where `assets/` sits.
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;
use vitality::ability::def_file::ability_def_from_file;
use vitality::actors::ActorBuilder;
use vitality::context::Vitality;
use vitality::effect::DefNames;
use vitality::effect::def_file::effect_def_from_file;
use vitality::prelude::*;
use vitality::registry::RegistryMut;
use vitality::registry::ability_registry::AbilityToken;
use vitality::registry::effect_registry::EffectToken;
use vitality::{VitalityPlugin, attribute, tag};

attribute!(Health, f32);
attribute!(Armor, f32);

tag!(Burning);
tag!(Fire);

pub const BURN: EffectToken = EffectToken::new_static("example.burn");
pub const IMMOLATE: AbilityToken = AbilityToken::new_static("example.immolate");

/// bevy's default fixed timestep, so one `update` is exactly one tick
const STEP: Duration = Duration::from_micros(15_625);
const TICKS: usize = 64 * 8;

fn main() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        VitalityPlugin::new()
            .attribute::<Health>()
            .attribute::<Armor>(),
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
    app.init_resource::<Printed>();
    app.add_systems(Startup, (declare_names, load_defs, spawn_actor).chain());
    app.add_systems(Update, report);

    for _ in 0..TICKS {
        app.update();
    }
}

/// a def file names attributes and tags, and only Rust can declare them
fn declare_names(mut names: ResMut<DefNames>) {
    names.tag::<Burning>();
    names.tag::<Fire>();
}

fn load_defs(mut registry: RegistryMut, names: Res<DefNames>) {
    let burn =
        effect_def_from_file("assets/defs/burn.effect", &names).expect("assets/defs/burn.effect");
    println!(
        "burn.effect: {} modifiers, {} tag, periodic {}",
        burn.modifiers.len(),
        burn.granted_tags.len(),
        burn.application_policy.is_periodic()
    );
    registry.add_effect(BURN, burn);

    let immolate = ability_def_from_file("assets/defs/immolate.ability", &names)
        .expect("assets/defs/immolate.ability");
    println!(
        "immolate.ability: {}, {} tags",
        immolate.name,
        immolate.tags.len()
    );
    registry.add_ability(IMMOLATE, immolate);
}

fn spawn_actor(mut vitality: Vitality) {
    let actor = ActorBuilder::new()
        .name("Hero")
        .with::<Health>(100.0)
        .with::<Armor>(10.0)
        .with_effect(&vitality.registry.effect(&BURN))
        .grant_ability(&vitality.registry.ability(&IMMOLATE))
        .build();
    vitality.add_spawn_actor(actor);
}

#[derive(Resource, Default)]
struct Printed(u32);

fn report(
    time: Res<Time>,
    mut printed: ResMut<Printed>,
    health: Single<&Health>,
    armor: Single<&Armor>,
    burning: Query<&Burning>,
) {
    let second = time.elapsed_secs() as u32;
    if second == printed.0 {
        return;
    }
    printed.0 = second;
    println!(
        "{second}s: H {} A {}{}",
        health.current_value(),
        armor.current_value(),
        if burning.is_empty() { "" } else { " [Burning]" }
    );
}
