//! The install surface, headless: bevy refuses a second add of a unique plugin before any build
//! guard runs, so every vitality plugin answers `is_unique` with false and leaves the second add to
//! `first_install`.

use bevy::prelude::*;
use std::any::TypeId;
use vitality::ability::AbilityDef;
use vitality::ability::AbilityPlugin;
use vitality::actors::ActorDef;
use vitality::attribute_set;
use vitality::effect::{ConditionPlugin, EffectsPlugin, GlobalEffectPlugin, GlobalEffects, Stacks};
use vitality::prelude::*;
use vitality::registry::ability_registry::AbilityRegistry;
use vitality::registry::effect_registry::EffectRegistry;
use vitality::{AttributesPlugin, VitalityPlugin};
use vitality_core::CorePlugin;
use vitality_core::registry::AttributeRegistry;

attribute!(Probe, f32);
attribute_set! { pub struct SoloSet { Solo: u32 } }

fn headless() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app
}

/// the whole install, proven by the resources only a `build` inserts
fn assert_built(app: &App) {
    assert!(app.is_plugin_added::<CorePlugin>(), "core is not installed");
    assert!(
        app.is_plugin_added::<AbilityPlugin>(),
        "abilities are not installed"
    );
    assert!(
        app.is_plugin_added::<ConditionPlugin>(),
        "conditions are not installed"
    );
    assert!(
        app.is_plugin_added::<EffectsPlugin>(),
        "effects are not installed"
    );
    assert!(
        app.is_plugin_added::<GlobalEffectPlugin>(),
        "the global effect marker is not installed"
    );
    assert!(
        app.world().contains_resource::<AttributeRegistry>(),
        "CorePlugin::build is what inserts AttributeRegistry"
    );
    assert!(
        app.world().contains_resource::<AbilityRegistry>()
            && app.world().contains_resource::<Assets<AbilityDef>>(),
        "AbilityPlugin::build is what inserts the ability registry and its asset store"
    );
    assert!(
        app.world().contains_resource::<EffectRegistry>()
            && app.world().contains_resource::<GlobalEffects>(),
        "EffectsPlugin::build and GlobalEffectPlugin::build are what insert those two"
    );
    assert!(
        app.world().contains_resource::<Assets<ActorDef>>(),
        "AttributesPlugin::build is what inits the actor definition store"
    );
    assert!(
        app.world()
            .resource::<AttributeRegistry>()
            .id_of(TypeId::of::<Stacks>())
            .is_some(),
        "{} is registered by AttributesPlugin::build, so the install is readable",
        std::any::type_name::<Stacks>()
    );
}

#[test]
fn ability_plugin_added_after_attributes_plugin() {
    let mut app = headless();
    app.add_plugins(AttributesPlugin);
    app.add_plugins(AbilityPlugin);

    assert_built(&app);
    app.update();
}

#[test]
fn ability_plugin_added_before_attributes_plugin() {
    let mut app = headless();
    app.add_plugins(AbilityPlugin);
    // AttributesPlugin::build adds AbilityPlugin itself, and bevy would call that a duplicate
    app.add_plugins(AttributesPlugin);

    assert_built(&app);
    app.update();
}

#[test]
fn attributes_plugin_added_twice_leaves_the_first_build_alone() {
    let mut app = headless();
    app.add_plugins(AttributesPlugin);
    app.add_plugins(AttributesPlugin);

    assert_built(&app);
    app.update();

    // drop what a build inserts, so a build that ran again would put it back
    app.world_mut().remove_resource::<AbilityRegistry>();
    app.add_plugins(AbilityPlugin);
    assert!(
        app.world().get_resource::<AbilityRegistry>().is_none(),
        "AbilityPlugin::build inserts that registry, so the guarded build must have no-op"
    );
}

#[test]
fn attributes_and_ability_plugin_in_one_tuple() {
    let mut app = headless();
    app.add_plugins((AttributesPlugin, AbilityPlugin));

    assert_built(&app);
    app.update();
}

#[test]
fn every_sub_plugin_added_before_attributes_plugin() {
    let mut app = headless();
    app.add_plugins((
        CorePlugin,
        AbilityPlugin,
        ConditionPlugin,
        EffectsPlugin,
        GlobalEffectPlugin::default(),
    ));
    app.add_plugins(AttributesPlugin);

    assert_built(&app);
    app.update();
}

#[test]
fn vitality_plugin_added_twice_registers_each_attribute_once() {
    let mut app = headless();
    app.add_plugins(VitalityPlugin::new().attribute::<Probe>());
    app.add_plugins(VitalityPlugin::new().attribute::<Probe>());

    assert_built(&app);
    assert_eq!(
        app.world().resource::<AttributeRegistry>().len(),
        2,
        "Stacks plus Probe: the second VitalityPlugin registered nothing again"
    );
    app.update();
}

#[test]
fn a_set_member_registered_directly_as_well_registers_once() {
    let mut app = headless();
    // the member reaches init_attribute through both paths at once
    app.add_plugins(VitalityPlugin::new().set::<SoloSet>().attribute::<Solo>());
    app.add_plugins(VitalityPlugin::new().set::<SoloSet>().attribute::<Solo>());

    assert_built(&app);
    assert_eq!(
        app.world().resource::<AttributeRegistry>().len(),
        2,
        "Stacks plus Solo: the set and the direct registration share one slot"
    );
    app.update();
}

#[test]
fn a_game_attribute_survives_a_second_install() {
    let mut app = headless();
    app.add_plugins(VitalityPlugin::new().attribute::<Probe>());
    app.add_plugins(AttributesPlugin);

    assert!(
        app.world()
            .resource::<AttributeRegistry>()
            .id_of(TypeId::of::<Probe>())
            .is_some(),
        "the host's attribute is still registered after the second install path runs"
    );
    assert_eq!(
        app.world().resource::<AttributeRegistry>().len(),
        2,
        "Probe and Stacks, one slot each"
    );
}
