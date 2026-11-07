pub mod debug_overlay;

use crate::inspector::debug_overlay::{explore_actors_system, setup_debug_overlay};

use crate::schedule::EffectsSet;
use bevy::prelude::*;

pub struct ActorInspectorPlugin;

impl Plugin for ActorInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_debug_overlay);
        app.add_systems(
            Update,
            explore_actors_system.in_set(EffectsSet::Notify), //.run_if(on_timer(Duration::from_millis(32))),
        );
    }
}

pub fn pretty_type_name<T>() -> String {
    format!("{}", ShortName::of::<T>())
}

pub fn pretty_type_name_str(val: &str) -> String {
    format!("{}", ShortName(val))
}
