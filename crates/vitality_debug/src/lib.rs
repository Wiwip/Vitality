pub mod debug_overlay;

use crate::debug_overlay::{explore_actors_system, setup_debug_overlay};
use bevy::prelude::*;
use vitality_core::schedule::EffectsSet;

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
